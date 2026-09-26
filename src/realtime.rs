//! In-process fan-out of realtime events to long-lived subscribers (SSE
//! streams), mirroring echobackend's `internal/platform/realtime.Hub`.
//!
//! Every instance keeps its subscribers in memory. Once [`Hub::start_relay`]
//! runs (Redis configured), events are published through Redis pub/sub on
//! `<CACHE_KEY_PREFIX>:realtime:<topic>`, the same channels echobackend uses,
//! and each instance delivers what its pattern subscription receives, so
//! events reach subscribers connected to any instance. Without Redis,
//! delivery is local only, which is correct for a single instance.

use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::sync::{LazyLock, OnceLock};
use std::time::Duration;
use tokio::sync::{broadcast, mpsc};
use tokio_stream::StreamExt;

use crate::cache::Cache;

/// How many undelivered events a subscriber may lag behind before it is
/// dropped.
const SUBSCRIPTION_BUFFER: usize = 64;

/// Bounds the wait between attempts to re-open the Redis subscription.
const RELAY_MAX_BACKOFF: Duration = Duration::from_secs(30);

type Topics = HashMap<String, broadcast::Sender<Arc<str>>>;

#[derive(Clone, Default)]
pub struct Hub {
    topics: Arc<Mutex<Topics>>,
    relay: Arc<OnceLock<Relay>>,
}

struct Relay {
    /// Feeds one publisher task, which keeps events in publish order.
    outgoing: mpsc::UnboundedSender<(String, Arc<str>)>,
}

static HUB: LazyLock<Hub> = LazyLock::new(Hub::default);

/// The process-wide hub.
pub fn hub() -> &'static Hub {
    &HUB
}

impl Hub {
    /// Registers a subscriber for `topic`.
    pub fn subscribe(&self, topic: &str) -> Subscription {
        let mut topics = self.topics.lock().unwrap_or_else(|e| e.into_inner());
        let rx = topics
            .entry(topic.to_string())
            .or_insert_with(|| broadcast::channel(SUBSCRIPTION_BUFFER).0)
            .subscribe();
        Subscription {
            rx,
            topic: topic.to_string(),
            hub: self.clone(),
        }
    }

    /// Serializes `event` and sends it to every subscriber of `topic`, across
    /// instances when the Redis relay runs. Having no subscribers is not an
    /// error.
    pub fn publish<T: Serialize>(&self, topic: &str, event: &T) -> Result<(), serde_json::Error> {
        let payload: Arc<str> = serde_json::to_string(event)?.into();
        match self.relay.get() {
            Some(relay) => {
                if let Err(mpsc::error::SendError((topic, payload))) =
                    relay.outgoing.send((topic.to_string(), payload))
                {
                    self.deliver(&topic, payload);
                }
            }
            None => self.deliver(topic, payload),
        }
        Ok(())
    }

    /// Hands `payload` to this instance's subscribers of `topic`.
    fn deliver(&self, topic: &str, payload: Arc<str>) {
        let topics = self.topics.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(tx) = topics.get(topic) {
            let _ = tx.send(payload);
        }
    }

    /// Routes events through Redis pub/sub from now on. Spawns the publisher
    /// and subscriber tasks on the current Tokio runtime; call it once, at
    /// startup, after [`crate::cache::init`] succeeded.
    pub fn start_relay(&self, cache: &'static Cache) {
        let prefix = format!("{}:", cache.build_key(&["realtime"]));
        let (outgoing, rx) = mpsc::unbounded_channel();
        // Whether the pattern subscription is live. While it is down, events
        // published by this instance are also delivered locally, since they
        // would otherwise never come back from Redis.
        let subscribed = Arc::new(AtomicBool::new(false));
        if self.relay.set(Relay { outgoing }).is_err() {
            tracing::warn!("realtime: Redis relay already started");
            return;
        }

        tokio::spawn(run_publisher(
            self.clone(),
            cache,
            prefix.clone(),
            rx,
            subscribed.clone(),
        ));
        tokio::spawn(run_subscriber(self.clone(), cache, prefix, subscribed));
        tracing::info!("realtime: relaying events through Redis pub/sub");
    }
}

async fn run_publisher(
    hub: Hub,
    cache: &'static Cache,
    prefix: String,
    mut rx: mpsc::UnboundedReceiver<(String, Arc<str>)>,
    subscribed: Arc<AtomicBool>,
) {
    while let Some((topic, payload)) = rx.recv().await {
        let channel = format!("{prefix}{topic}");
        match cache.publish(&channel, payload.as_bytes()).await {
            Ok(()) if subscribed.load(Ordering::Acquire) => {}
            // Redis accepted it but this instance's subscription is down,
            // so it would not come back to local subscribers.
            Ok(()) => hub.deliver(&topic, payload),
            Err(err) => {
                tracing::warn!(error = %err, %topic, "realtime: Redis publish failed, delivering locally");
                hub.deliver(&topic, payload);
            }
        }
    }
}

/// Keeps one pattern subscription open, re-opening it with backoff whenever
/// the connection drops.
async fn run_subscriber(
    hub: Hub,
    cache: &'static Cache,
    prefix: String,
    subscribed: Arc<AtomicBool>,
) {
    let pattern = format!("{prefix}*");
    let mut backoff = Duration::from_secs(1);
    loop {
        match cache.pubsub().await {
            Ok(mut pubsub) => match pubsub.psubscribe(&pattern).await {
                Ok(()) => {
                    subscribed.store(true, Ordering::Release);
                    backoff = Duration::from_secs(1);
                    let mut messages = pubsub.into_on_message();
                    while let Some(msg) = messages.next().await {
                        let Some(topic) = msg.get_channel_name().strip_prefix(&prefix) else {
                            continue;
                        };
                        match std::str::from_utf8(msg.get_payload_bytes()) {
                            Ok(payload) => hub.deliver(topic, payload.into()),
                            Err(_) => {
                                tracing::warn!(%topic, "realtime: dropping non-UTF-8 event");
                            }
                        }
                    }
                    subscribed.store(false, Ordering::Release);
                    tracing::warn!("realtime: Redis subscription closed, reconnecting");
                }
                Err(err) => tracing::warn!(error = %err, "realtime: Redis psubscribe failed"),
            },
            Err(err) => tracing::warn!(error = %err, "realtime: Redis pub/sub connect failed"),
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(RELAY_MAX_BACKOFF);
    }
}

/// Receives the events published to one topic. The topic is released once its
/// last subscription is dropped.
pub struct Subscription {
    rx: broadcast::Receiver<Arc<str>>,
    topic: String,
    hub: Hub,
}

impl Subscription {
    /// Waits for the next event. `None` means the subscription ended because
    /// the subscriber fell too far behind; it should reconnect and refetch
    /// history.
    pub async fn recv(&mut self) -> Option<Arc<str>> {
        self.rx.recv().await.ok()
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        let mut topics = self.hub.topics.lock().unwrap_or_else(|e| e.into_inner());
        // `self.rx` is still alive here, so the last subscriber sees a count of 1.
        if topics
            .get(&self.topic)
            .is_some_and(|tx| tx.receiver_count() <= 1)
        {
            topics.remove(&self.topic);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_delivers_only_to_topic_subscribers() {
        let hub = Hub::default();
        let mut a = hub.subscribe("guild:a");
        let mut b = hub.subscribe("guild:b");

        hub.publish("guild:a", &serde_json::json!({ "type": "x" }))
            .unwrap();

        assert_eq!(a.recv().await.as_deref(), Some(r#"{"type":"x"}"#));
        assert!(b.rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_topic_released_after_last_subscriber() {
        let hub = Hub::default();
        let first = hub.subscribe("t");
        let second = hub.subscribe("t");
        drop(first);
        assert!(hub.topics.lock().unwrap().contains_key("t"));
        drop(second);
        assert!(!hub.topics.lock().unwrap().contains_key("t"));
    }

    #[tokio::test]
    async fn test_drops_slow_subscriber() {
        let hub = Hub::default();
        let mut slow = hub.subscribe("t");
        for i in 0..=SUBSCRIPTION_BUFFER {
            hub.publish("t", &i).unwrap();
        }
        assert!(slow.recv().await.is_none());
    }
}
