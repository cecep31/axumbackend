//! In-process fan-out of realtime events to long-lived subscribers (SSE
//! streams), mirroring echobackend's `internal/platform/realtime.Hub`.
//!
//! Delivery is local to this instance: echobackend relays through Redis
//! pub/sub when it is configured, but axumbackend has no Redis, so events only
//! reach subscribers connected to the instance that published them.

use once_cell::sync::Lazy;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

/// How many undelivered events a subscriber may lag behind before it is
/// dropped.
const SUBSCRIPTION_BUFFER: usize = 64;

type Topics = HashMap<String, broadcast::Sender<Arc<str>>>;

#[derive(Clone, Default)]
pub struct Hub {
    topics: Arc<Mutex<Topics>>,
}

static HUB: Lazy<Hub> = Lazy::new(Hub::default);

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

    /// Serializes `event` and sends it to every subscriber of `topic`. Having
    /// no subscribers is not an error.
    pub fn publish<T: Serialize>(&self, topic: &str, event: &T) -> Result<(), serde_json::Error> {
        let payload: Arc<str> = serde_json::to_string(event)?.into();
        let topics = self.topics.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(tx) = topics.get(topic) {
            let _ = tx.send(payload);
        }
        Ok(())
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
