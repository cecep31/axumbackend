//! Optional Redis/Valkey client, mirroring echobackend's
//! `internal/platform/cache.RedisCache`.
//!
//! The cache is fail-open: when `REDIS_URL` is empty or the server cannot be
//! reached at startup, [`get`] returns `None` and every caller falls back to
//! its in-memory behaviour. Keys use the same `<CACHE_KEY_PREFIX>:<parts…>`
//! layout as echobackend, so both backends can share one Redis.

use std::sync::OnceLock;
use std::time::Duration;

use redis::aio::{ConnectionManager, ConnectionManagerConfig, PubSub};
use redis::{Client, RedisResult, Script};
use serde::{Serialize, de::DeserializeOwned};

use crate::config::CacheConfig;

/// Startup fails fast when Redis is unreachable: it is optional, so a slow
/// connect should not hold up the server. Matches echobackend's 2s cap.
const MAX_CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

/// Increments a fixed-window counter, starting the window on the first hit.
/// Returns `{count, remaining_ttl_ms}`. Same script as echobackend.
const FIXED_WINDOW_INCREMENT_SCRIPT: &str = r#"
local current = redis.call("INCR", KEYS[1])
if current == 1 then
	redis.call("PEXPIRE", KEYS[1], ARGV[1])
end
local ttl = redis.call("PTTL", KEYS[1])
return {current, ttl}
"#;

static CACHE: OnceLock<Cache> = OnceLock::new();
static FIXED_WINDOW_SCRIPT: OnceLock<Script> = OnceLock::new();

pub struct Cache {
    client: Client,
    conn: ConnectionManager,
    key_prefix: String,
    ttl: Duration,
    timeout: Duration,
}

/// The process-wide cache, or `None` when Redis is disabled or unreachable.
pub fn get() -> Option<&'static Cache> {
    CACHE.get()
}

/// Connects to Redis and publishes the client for [`get`]. Returns the cache,
/// or `None` when it is disabled. Must be called at most once, at startup.
pub async fn init(cfg: &CacheConfig) -> Option<&'static Cache> {
    if cfg.valkey_url.is_empty() {
        tracing::info!("Redis cache disabled (REDIS_URL not set)");
        return None;
    }

    // Both rustls backends are compiled in, so `rediss://` needs an explicit
    // process-wide provider. Another crate may already have installed one.
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let client = match Client::open(cfg.valkey_url.as_str()) {
        Ok(client) => client,
        Err(err) => {
            tracing::warn!(error = %err, "cache: invalid REDIS_URL, caching disabled");
            return None;
        }
    };

    let timeout = if cfg.connect_timeout.is_zero() {
        MAX_CONNECT_TIMEOUT
    } else {
        cfg.connect_timeout.min(MAX_CONNECT_TIMEOUT)
    };
    let manager_config = ConnectionManagerConfig::new()
        .set_connection_timeout(Some(timeout))
        .set_response_timeout(Some(timeout));

    let conn = match tokio::time::timeout(
        timeout,
        ConnectionManager::new_with_config(client.clone(), manager_config),
    )
    .await
    {
        Ok(Ok(conn)) => conn,
        Ok(Err(err)) => {
            tracing::warn!(error = %err, "cache: failed to connect to Redis, caching disabled");
            return None;
        }
        Err(_) => {
            tracing::warn!(?timeout, "cache: Redis connect timed out, caching disabled");
            return None;
        }
    };

    let key_prefix = cfg.key_prefix.trim().to_string();
    if cfg.ttl.is_zero() {
        tracing::warn!("cache: CACHE_TTL_SECONDS is 0, default-TTL writes are skipped");
    }
    tracing::info!(ttl = ?cfg.ttl, %key_prefix, "cache: connected to Redis");

    if CACHE
        .set(Cache {
            client,
            conn,
            key_prefix,
            ttl: cfg.ttl,
            timeout,
        })
        .is_err()
    {
        tracing::warn!("cache: already initialized");
    }
    CACHE.get()
}

impl Cache {
    /// Joins `parts` with `:` under the configured key prefix.
    pub fn build_key(&self, parts: &[&str]) -> String {
        build_key(&self.key_prefix, parts)
    }

    /// Reads a JSON value. A value that no longer decodes is reported as a
    /// miss so the caller simply overwrites it.
    pub async fn get_json<T: DeserializeOwned>(&self, key: &str) -> RedisResult<Option<T>> {
        let raw: Option<Vec<u8>> = redis::cmd("GET")
            .arg(key)
            .query_async(&mut self.conn.clone())
            .await?;
        Ok(decode(key, raw))
    }

    /// Reads and deletes a JSON value in one step (`GETDEL`), so a one-time
    /// value can be redeemed only once across instances.
    pub async fn take_json<T: DeserializeOwned>(&self, key: &str) -> RedisResult<Option<T>> {
        let raw: Option<Vec<u8>> = redis::cmd("GETDEL")
            .arg(key)
            .query_async(&mut self.conn.clone())
            .await?;
        Ok(decode(key, raw))
    }

    /// Writes a JSON value with the default `CACHE_TTL_SECONDS`. A zero TTL
    /// disables the write.
    pub async fn set_json<T: Serialize>(&self, key: &str, value: &T) -> RedisResult<()> {
        self.set_json_with_ttl(key, value, self.ttl).await
    }

    pub async fn set_json_with_ttl<T: Serialize>(
        &self,
        key: &str,
        value: &T,
        ttl: Duration,
    ) -> RedisResult<()> {
        let ttl_ms = ttl.as_millis() as u64;
        if ttl_ms == 0 {
            return Ok(());
        }
        let payload = match serde_json::to_vec(value) {
            Ok(payload) => payload,
            Err(err) => {
                tracing::warn!(key, error = %err, "cache: failed to encode value");
                return Ok(());
            }
        };
        redis::cmd("SET")
            .arg(key)
            .arg(payload)
            .arg("PX")
            .arg(ttl_ms)
            .query_async(&mut self.conn.clone())
            .await
    }

    /// Counts one hit in the fixed window stored at `key`. Returns the hit
    /// count so far and the time left in the window.
    pub async fn increment_fixed_window(
        &self,
        key: &str,
        window: Duration,
    ) -> RedisResult<(u64, Duration)> {
        let script = FIXED_WINDOW_SCRIPT.get_or_init(|| Script::new(FIXED_WINDOW_INCREMENT_SCRIPT));
        let (count, ttl_ms): (i64, i64) = script
            .key(key)
            .arg(window.as_millis().max(1) as u64)
            .invoke_async(&mut self.conn.clone())
            .await?;
        Ok((
            count.max(0) as u64,
            Duration::from_millis(ttl_ms.max(0) as u64),
        ))
    }

    pub async fn publish(&self, channel: &str, payload: &[u8]) -> RedisResult<()> {
        redis::cmd("PUBLISH")
            .arg(channel)
            .arg(payload)
            .query_async(&mut self.conn.clone())
            .await
    }

    /// Opens a dedicated pub/sub connection. Unlike the shared connection it
    /// does not reconnect by itself; the caller owns the retry loop.
    pub async fn pubsub(&self) -> RedisResult<PubSub> {
        match tokio::time::timeout(self.timeout, self.client.get_async_pubsub()).await {
            Ok(result) => result,
            Err(_) => Err(redis::RedisError::from(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "pub/sub connect timed out",
            ))),
        }
    }
}

fn build_key(prefix: &str, parts: &[&str]) -> String {
    let joined = parts.join(":");
    if prefix.is_empty() {
        joined
    } else {
        format!("{prefix}:{joined}")
    }
}

fn decode<T: DeserializeOwned>(key: &str, raw: Option<Vec<u8>>) -> Option<T> {
    let raw = raw?;
    match serde_json::from_slice(&raw) {
        Ok(value) => Some(value),
        Err(err) => {
            tracing::warn!(key, error = %err, "cache: failed to decode cached value");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_key_with_prefix() {
        assert_eq!(
            build_key("pilput", &["posts", "random", "9"]),
            "pilput:posts:random:9"
        );
    }

    #[test]
    fn test_build_key_without_prefix() {
        assert_eq!(
            build_key("", &["rate_limit", "auth:login", "1.2.3.4"]),
            "rate_limit:auth:login:1.2.3.4"
        );
    }

    /// Needs a live server: `REDIS_URL=redis://127.0.0.1:6379 cargo test -- --ignored`
    #[tokio::test]
    #[ignore]
    async fn test_against_live_redis() {
        let cfg = CacheConfig {
            valkey_url: std::env::var("REDIS_URL").expect("REDIS_URL must be set"),
            key_prefix: format!("axumbackend-test-{}", uuid::Uuid::now_v7()),
            ttl: Duration::from_secs(5),
            connect_timeout: Duration::from_secs(2),
        };
        let cache = init(&cfg).await.expect("Redis should be reachable");

        let key = cache.build_key(&["json"]);
        cache.set_json(&key, &vec![1, 2, 3]).await.unwrap();
        assert_eq!(
            cache.get_json::<Vec<i32>>(&key).await.unwrap(),
            Some(vec![1, 2, 3])
        );
        assert_eq!(
            cache.take_json::<Vec<i32>>(&key).await.unwrap(),
            Some(vec![1, 2, 3])
        );
        assert_eq!(cache.take_json::<Vec<i32>>(&key).await.unwrap(), None);

        let key = cache.build_key(&["window"]);
        let window = Duration::from_secs(5);
        let (first, ttl) = cache.increment_fixed_window(&key, window).await.unwrap();
        let (second, _) = cache.increment_fixed_window(&key, window).await.unwrap();
        assert_eq!((first, second), (1, 2));
        assert!(ttl > Duration::ZERO && ttl <= window);

        let mut pubsub = cache.pubsub().await.unwrap();
        let channel = cache.build_key(&["realtime", "t"]);
        pubsub.subscribe(&channel).await.unwrap();
        cache.publish(&channel, b"hello").await.unwrap();
        let msg = tokio_stream::StreamExt::next(&mut pubsub.on_message())
            .await
            .unwrap();
        assert_eq!(msg.get_payload_bytes(), b"hello");
    }

    #[test]
    fn test_decode_treats_bad_json_as_miss() {
        assert_eq!(decode::<Vec<u32>>("k", Some(b"not json".to_vec())), None);
        assert_eq!(
            decode::<Vec<u32>>("k", Some(b"[1,2]".to_vec())),
            Some(vec![1, 2])
        );
        assert_eq!(decode::<Vec<u32>>("k", None), None);
    }
}
