use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    Json,
    extract::{ConnectInfo, Request, State},
    http::{HeaderMap, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::{cache, response::ApiResponse};

const MAX_RATE_LIMITER_ENTRIES: usize = 20_000;

#[derive(Clone)]
pub struct RateLimiter {
    inner: Arc<Mutex<RateLimiterInner>>,
    max_requests: u32,
    window: Duration,
    trust_proxy: bool,
    /// Set by [`RateLimiter::shared`]: counts in Redis under
    /// `rate_limit:<name>:<client>` so every instance shares one limit.
    shared_name: Option<Arc<str>>,
}

struct RateLimiterInner {
    windows: HashMap<RateLimitKey, Window>,
    last_cleanup: Instant,
}

#[derive(Clone, Eq, Hash, PartialEq)]
struct RateLimitKey {
    path: String,
    client: String,
}

struct Window {
    started_at: Instant,
    requests: u32,
}

impl RateLimiter {
    pub fn new(max_requests: u32, window: Duration, trust_proxy: bool) -> Self {
        Self {
            inner: Arc::new(Mutex::new(RateLimiterInner {
                windows: HashMap::new(),
                last_cleanup: Instant::now(),
            })),
            max_requests,
            window,
            trust_proxy,
            shared_name: None,
        }
    }

    /// Keeps the counters in Redis, when it is available, under `name` (e.g.
    /// `auth:login`, as in echobackend), so the limit holds across instances.
    /// The limit then applies per client for every path behind this limiter.
    /// Falls back to the in-memory counters whenever Redis is off or failing.
    pub fn shared(mut self, name: &str) -> Self {
        self.shared_name = Some(name.into());
        self
    }

    /// Counts the hit in Redis. `None` means the shared store is unavailable
    /// and the caller should use the in-memory counters instead.
    async fn check_shared(&self, client: &str) -> Option<Result<(), u64>> {
        let name = self.shared_name.as_deref()?;
        let cache = cache::get()?;
        let key = cache.build_key(&["rate_limit", name, client]);
        match cache.increment_fixed_window(&key, self.window).await {
            Ok((0, _)) => None,
            Ok((count, _)) if count <= u64::from(self.max_requests) => Some(Ok(())),
            Ok((_, remaining)) => Some(Err(remaining.as_secs_f64().ceil().max(1.0) as u64)),
            Err(err) => {
                tracing::warn!(name, error = %err, "rate limit: Redis unavailable, using in-memory counters");
                None
            }
        }
    }

    fn check(&self, key: RateLimitKey) -> Result<(), u64> {
        let now = Instant::now();
        let mut inner = self.inner.lock().expect("rate limiter lock poisoned");

        // Periodically or upon reaching capacity, prune expired windows and shrink map capacity
        let cleanup_interval = self.window.min(Duration::from_secs(5));
        let should_cleanup = now.duration_since(inner.last_cleanup) >= cleanup_interval
            || inner.windows.len() >= MAX_RATE_LIMITER_ENTRIES;

        if should_cleanup {
            inner
                .windows
                .retain(|_, window| now.duration_since(window.started_at) < self.window);

            if inner.windows.capacity() > 128 && inner.windows.len() * 4 < inner.windows.capacity()
            {
                let min_cap = inner.windows.len().max(32);
                inner.windows.shrink_to(min_cap);
            }

            inner.last_cleanup = now;
        }

        // If map is still at capacity (e.g. active DDoS with many unique URLs/IPs),
        // evict the oldest window to ensure memory stays bounded
        if inner.windows.len() >= MAX_RATE_LIMITER_ENTRIES
            && !inner.windows.contains_key(&key)
            && let Some(oldest_key) = inner
                .windows
                .iter()
                .min_by_key(|(_, w)| w.started_at)
                .map(|(k, _)| k.clone())
        {
            inner.windows.remove(&oldest_key);
        }

        let window = inner.windows.entry(key).or_insert_with(|| Window {
            started_at: now,
            requests: 0,
        });

        if now.duration_since(window.started_at) >= self.window {
            window.started_at = now;
            window.requests = 0;
        }

        if window.requests >= self.max_requests {
            let retry_after = self
                .window
                .saturating_sub(now.duration_since(window.started_at))
                .as_secs()
                .max(1);
            return Err(retry_after);
        }

        window.requests += 1;
        Ok(())
    }
}

pub async fn rate_limit(
    State(limiter): State<RateLimiter>,
    request: Request,
    next: Next,
) -> Response {
    let client = client_identity(&request, limiter.trust_proxy);
    let result = match limiter.check_shared(&client).await {
        Some(result) => result,
        None => limiter.check(RateLimitKey {
            path: request.uri().path().to_owned(),
            client,
        }),
    };

    match result {
        Ok(()) => next.run(request).await,
        Err(retry_after) => {
            let body = Json(ApiResponse::error(
                "Too many attempts. Please try again later.",
                "Rate limit exceeded",
            ));

            let mut response = (StatusCode::TOO_MANY_REQUESTS, body).into_response();
            if let Ok(value) = retry_after.to_string().parse() {
                response.headers_mut().insert(header::RETRY_AFTER, value);
            }
            response
        }
    }
}

fn client_identity(request: &Request, trust_proxy: bool) -> String {
    if trust_proxy && let Some(ip) = forwarded_ip(request.headers()) {
        return ip.to_string();
    }

    if let Some(ConnectInfo(addr)) = request.extensions().get::<ConnectInfo<SocketAddr>>() {
        return addr.ip().to_string();
    }

    forwarded_ip(request.headers())
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn forwarded_ip(headers: &HeaderMap) -> Option<IpAddr> {
    headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .and_then(|value| value.trim().parse().ok())
        .or_else(|| {
            headers
                .get("x-real-ip")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.trim().parse().ok())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{HeaderValue, Request};
    use std::net::SocketAddr;
    use std::thread;

    #[test]
    fn test_client_identity_trust_proxy_enabled() {
        let socket_addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let mut req = Request::builder()
            .header("x-forwarded-for", "203.0.113.195, 70.41.3.18")
            .body(Body::empty())
            .unwrap();
        req.extensions_mut().insert(ConnectInfo(socket_addr));

        assert_eq!(client_identity(&req, true), "203.0.113.195");
    }

    #[test]
    fn test_client_identity_trust_proxy_disabled() {
        let socket_addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let mut req = Request::builder()
            .header("x-forwarded-for", "203.0.113.195, 70.41.3.18")
            .body(Body::empty())
            .unwrap();
        req.extensions_mut().insert(ConnectInfo(socket_addr));

        assert_eq!(client_identity(&req, false), "127.0.0.1");
    }

    #[test]
    fn test_forwarded_ip_x_real_ip_fallback() {
        let mut headers = HeaderMap::new();
        headers.insert("x-real-ip", HeaderValue::from_static("198.51.100.22"));

        assert_eq!(
            forwarded_ip(&headers),
            Some("198.51.100.22".parse().unwrap())
        );
    }

    #[test]
    fn test_forwarded_ip_invalid_and_empty() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("not-an-ip"));
        assert_eq!(forwarded_ip(&headers), None);

        let empty_headers = HeaderMap::new();
        assert_eq!(forwarded_ip(&empty_headers), None);
    }

    #[test]
    fn test_rate_limiter_check_and_block() {
        let limiter = RateLimiter::new(2, Duration::from_secs(60), false);
        let key = RateLimitKey {
            path: "/api/posts".into(),
            client: "192.168.1.1".into(),
        };

        // First request: ok
        assert!(limiter.check(key.clone()).is_ok());
        // Second request: ok
        assert!(limiter.check(key.clone()).is_ok());
        // Third request: blocked
        let res = limiter.check(key.clone());
        assert!(res.is_err());
        let retry_after = res.unwrap_err();
        assert!((1..=60).contains(&retry_after));
    }

    #[test]
    fn test_rate_limiter_independent_keys() {
        let limiter = RateLimiter::new(1, Duration::from_secs(60), false);
        let key1 = RateLimitKey {
            path: "/api/posts".into(),
            client: "192.168.1.1".into(),
        };
        let key2 = RateLimitKey {
            path: "/api/comments".into(),
            client: "192.168.1.1".into(),
        };
        let key3 = RateLimitKey {
            path: "/api/posts".into(),
            client: "192.168.1.2".into(),
        };

        assert!(limiter.check(key1.clone()).is_ok());
        assert!(limiter.check(key1.clone()).is_err());

        // Other path or other client are still allowed
        assert!(limiter.check(key2).is_ok());
        assert!(limiter.check(key3).is_ok());
    }

    #[test]
    fn test_rate_limiter_window_expiry() {
        let limiter = RateLimiter::new(1, Duration::from_millis(50), false);
        let key = RateLimitKey {
            path: "/api/ping".into(),
            client: "127.0.0.1".into(),
        };

        assert!(limiter.check(key.clone()).is_ok());
        assert!(limiter.check(key.clone()).is_err());

        thread::sleep(Duration::from_millis(60));

        // Should be allowed again after window expires
        assert!(limiter.check(key).is_ok());
    }

    #[test]
    fn test_rate_limiter_eviction_and_cleanup() {
        let limiter = RateLimiter::new(10, Duration::from_millis(50), false);
        for i in 0..50 {
            let key = RateLimitKey {
                path: format!("/api/posts/{}", i),
                client: "127.0.0.1".into(),
            };
            assert!(limiter.check(key).is_ok());
        }

        // Wait for window to expire
        thread::sleep(Duration::from_millis(60));

        // Next check triggers cleanup
        let new_key = RateLimitKey {
            path: "/api/posts/new".into(),
            client: "127.0.0.1".into(),
        };
        assert!(limiter.check(new_key).is_ok());

        let inner = limiter.inner.lock().unwrap();
        // Expired items should be pruned down
        assert!(inner.windows.len() <= 2);
    }
}
