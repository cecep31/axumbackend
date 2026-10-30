//! Rate limiters.
//!
//! - [`TokenBucketLimiter`] / [`global_rate_limit`]: the global
//!   `HTTP_RATE_LIMIT_RPS` limiter, mirroring echobackend's Echo
//!   `RateLimiter` memory store (token bucket per client IP, `rate = rps`,
//!   `burst = 2 * rps`, idle buckets expire after `HTTP_RATE_LIMIT_WINDOW_SEC`
//!   or 3 minutes). It sits inside the CORS layer, so preflights are answered
//!   first and 429s carry CORS headers.
//! - [`RateLimiter`] / [`rate_limit`]: fixed-window per-route limiters (auth,
//!   post create/report). With [`RateLimiter::shared`] they count in Redis
//!   under `rate_limit:<name>:<client>` like echobackend.
//!
//! Both answer 429 with `{success:false, message:"Too many attempts. Please
//! try again later.", error:"Rate limit exceeded"}` and `Retry-After`.

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    Json,
    extract::{ConnectInfo, Request, State},
    http::{StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::{
    http::{client_ip::resolve_client_ip, response::ApiResponse},
    infra::cache,
};

const MAX_RATE_LIMITER_ENTRIES: usize = 20_000;

/// The shared 429 envelope (echobackend `response.TooManyRequests`).
pub fn too_many_requests(retry_after_secs: u64) -> Response {
    let body = Json(ApiResponse::error(
        "Too many attempts. Please try again later.",
        "Rate limit exceeded",
    ));
    let mut response = (StatusCode::TOO_MANY_REQUESTS, body).into_response();
    if let Ok(value) = retry_after_secs.max(1).to_string().parse() {
        response.headers_mut().insert(header::RETRY_AFTER, value);
    }
    response
}

/// Client key for limiters: the resolved client IP, or `"unknown"`.
fn client_key(request: &Request, trust_proxy: bool) -> String {
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(addr)| addr.ip());
    resolve_client_ip(request.headers(), peer, trust_proxy)
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

// ============================================================================
// Global token bucket (echobackend RateLimiterMemoryStore)
// ============================================================================

#[derive(Clone)]
pub struct TokenBucketLimiter {
    inner: Arc<Mutex<TokenBucketInner>>,
    rate: f64,
    burst: f64,
    idle_expiry: Duration,
    trust_proxy: bool,
}

struct TokenBucketInner {
    visitors: HashMap<String, Bucket>,
    last_cleanup: Instant,
}

struct Bucket {
    tokens: f64,
    updated_at: Instant,
    last_seen: Instant,
}

impl TokenBucketLimiter {
    /// `rps` tokens per second with a burst of `2 * rps`; an idle client's
    /// bucket is dropped after `idle_expiry`.
    pub fn new(rps: u32, idle_expiry: Duration, trust_proxy: bool) -> Self {
        let rate = f64::from(rps.max(1));
        Self {
            inner: Arc::new(Mutex::new(TokenBucketInner {
                visitors: HashMap::new(),
                last_cleanup: Instant::now(),
            })),
            rate,
            burst: rate * 2.0,
            idle_expiry,
            trust_proxy,
        }
    }

    /// Takes one token for `client`; `Err(retry_after_secs)` when empty.
    fn check(&self, client: &str, now: Instant) -> Result<(), u64> {
        let mut inner = self.inner.lock().expect("rate limiter lock poisoned");

        if now.duration_since(inner.last_cleanup) > self.idle_expiry
            || inner.visitors.len() >= MAX_RATE_LIMITER_ENTRIES
        {
            let idle_expiry = self.idle_expiry;
            inner
                .visitors
                .retain(|_, bucket| now.duration_since(bucket.last_seen) <= idle_expiry);
            inner.last_cleanup = now;
        }

        if inner.visitors.len() >= MAX_RATE_LIMITER_ENTRIES
            && !inner.visitors.contains_key(client)
            && let Some(oldest) = inner
                .visitors
                .iter()
                .min_by_key(|(_, bucket)| bucket.last_seen)
                .map(|(key, _)| key.clone())
        {
            inner.visitors.remove(&oldest);
        }

        let burst = self.burst;
        let bucket = inner
            .visitors
            .entry(client.to_owned())
            .or_insert_with(|| Bucket {
                tokens: burst,
                updated_at: now,
                last_seen: now,
            });
        let elapsed = now.duration_since(bucket.updated_at).as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * self.rate).min(self.burst);
        bucket.updated_at = now;
        bucket.last_seen = now;

        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            Ok(())
        } else {
            Err(((1.0 - bucket.tokens) / self.rate).ceil().max(1.0) as u64)
        }
    }
}

/// Global limiter middleware (`from_fn_with_state(TokenBucketLimiter, ..)`).
pub async fn global_rate_limit(
    State(limiter): State<TokenBucketLimiter>,
    request: Request,
    next: Next,
) -> Response {
    let client = client_key(&request, limiter.trust_proxy);
    match limiter.check(&client, Instant::now()) {
        Ok(()) => next.run(request).await,
        Err(retry_after) => too_many_requests(retry_after),
    }
}

// ============================================================================
// Fixed-window per-route limiter
// ============================================================================

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
    /// The limit then applies per client for every path behind this limiter,
    /// including the in-memory fallback used whenever Redis is off or failing
    /// (so `/api/posts/{id}/reports` is limited per IP, not per post).
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
            // echobackend: `max(int(retryAfter.Seconds()), 1)` (floor, min 1).
            Ok((_, remaining)) => Some(Err(remaining.as_secs().max(1))),
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
    let client = client_key(&request, limiter.trust_proxy);
    let result = match limiter.check_shared(&client).await {
        Some(result) => result,
        None => limiter.check(RateLimitKey {
            // Shared limiters count per client across all their paths.
            path: if limiter.shared_name.is_some() {
                String::new()
            } else {
                request.uri().path().to_owned()
            },
            client,
        }),
    };

    match result {
        Ok(()) => next.run(request).await,
        Err(retry_after) => too_many_requests(retry_after),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use std::net::SocketAddr;
    use std::thread;

    #[test]
    fn test_token_bucket_burst_then_refill() {
        let limiter = TokenBucketLimiter::new(2, Duration::from_secs(180), false);
        let start = Instant::now();
        // burst = 2 * rps
        for _ in 0..4 {
            assert!(limiter.check("1.1.1.1", start).is_ok());
        }
        assert_eq!(limiter.check("1.1.1.1", start), Err(1));
        // Other clients have their own bucket.
        assert!(limiter.check("2.2.2.2", start).is_ok());
        // rate = rps tokens per second.
        let later = start + Duration::from_millis(500);
        assert!(limiter.check("1.1.1.1", later).is_ok());
        assert!(limiter.check("1.1.1.1", later).is_err());
    }

    #[test]
    fn test_token_bucket_drops_idle_visitors() {
        let limiter = TokenBucketLimiter::new(1, Duration::from_millis(50), false);
        let start = Instant::now();
        assert!(limiter.check("1.1.1.1", start).is_ok());
        assert!(
            limiter
                .check("2.2.2.2", start + Duration::from_millis(100))
                .is_ok()
        );
        let inner = limiter.inner.lock().unwrap();
        assert_eq!(inner.visitors.len(), 1);
    }

    #[tokio::test]
    async fn test_global_limiter_returns_envelope_with_retry_after() {
        use axum::{Router, middleware, routing::get};
        use tower::ServiceExt;

        let limiter = TokenBucketLimiter::new(1, Duration::from_secs(180), false);
        let app = Router::new()
            .route("/x", get(|| async { "ok" }))
            .layer(middleware::from_fn_with_state(limiter, global_rate_limit));
        let mut last = None;
        for _ in 0..3 {
            let mut req = Request::builder().uri("/x").body(Body::empty()).unwrap();
            req.extensions_mut()
                .insert(ConnectInfo("10.0.0.1:1".parse::<SocketAddr>().unwrap()));
            last = Some(app.clone().oneshot(req).await.unwrap());
        }
        let res = last.unwrap();
        assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(res.headers()[header::RETRY_AFTER], "1");
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            body,
            serde_json::json!({
                "success": false,
                "message": "Too many attempts. Please try again later.",
                "error": "Rate limit exceeded",
            })
        );
    }

    /// echobackend order on `POST /api/posts` / `POST /api/posts/{id}/reports`:
    /// `Auth()` runs before the limiter, so 401s never consume the budget.
    #[tokio::test]
    async fn test_auth_runs_before_route_limiter() {
        use crate::http::auth::{AuthUser, require_auth};
        use axum::{Router, middleware, routing::post};
        use tower::ServiceExt;

        let limiter = RateLimiter::new(1, Duration::from_secs(3600), false).shared("test:auth");
        let app = Router::new().route(
            "/x",
            post(|_user: AuthUser| async { "ok" })
                .route_layer(middleware::from_fn_with_state(limiter, rate_limit))
                .route_layer(middleware::from_fn(require_auth)),
        );
        let request = |authenticated: bool| {
            let mut req = Request::builder()
                .method("POST")
                .uri("/x")
                .body(Body::empty())
                .unwrap();
            req.extensions_mut()
                .insert(ConnectInfo("10.0.0.9:1".parse::<SocketAddr>().unwrap()));
            if authenticated {
                // Stands in for a verified Bearer token.
                req.extensions_mut().insert(AuthUser {
                    id: uuid::Uuid::now_v7(),
                    is_super_admin: false,
                });
            }
            req
        };

        for _ in 0..3 {
            let res = app.clone().oneshot(request(false)).await.unwrap();
            assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        }
        let res = app.clone().oneshot(request(true)).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let res = app.clone().oneshot(request(true)).await.unwrap();
        assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
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

    /// Sends 6 logins from one client (`X-Forwarded-For`) that arrive through
    /// a different Railway edge proxy each time, against a limit of 3.
    async fn statuses_behind_rotating_proxies(trust_proxy: bool, xff: &str) -> Vec<u16> {
        use axum::{Router, middleware, routing::post};
        use tower::ServiceExt;

        let limiter = RateLimiter::new(3, Duration::from_secs(60), trust_proxy);
        let app = Router::new().route(
            "/login",
            post(|| async { "ok" })
                .route_layer(middleware::from_fn_with_state(limiter, rate_limit)),
        );
        let mut codes = vec![];
        for i in 0..6 {
            let mut req = Request::builder()
                .method("POST")
                .uri("/login")
                .header("x-forwarded-for", xff)
                .body(Body::empty())
                .unwrap();
            let peer: SocketAddr = format!("100.64.0.{}:4000", i + 1).parse().unwrap();
            req.extensions_mut().insert(ConnectInfo(peer));
            codes.push(app.clone().oneshot(req).await.unwrap().status().as_u16());
        }
        codes
    }

    #[tokio::test]
    async fn test_middleware_limits_client_behind_rotating_proxies() {
        assert_eq!(
            statuses_behind_rotating_proxies(true, "203.0.113.50").await,
            vec![200, 200, 200, 429, 429, 429]
        );
        // A forged leading entry does not buy a fresh counter.
        assert_eq!(
            statuses_behind_rotating_proxies(true, "9.9.9.9, 203.0.113.50").await,
            vec![200, 200, 200, 429, 429, 429]
        );
    }

    /// Without `HTTP_TRUST_PROXY` every edge proxy counts as its own client,
    /// so the limit is never reached behind a load-balanced proxy.
    #[tokio::test]
    async fn test_middleware_without_trust_proxy_counts_per_proxy() {
        assert_eq!(
            statuses_behind_rotating_proxies(false, "203.0.113.50").await,
            vec![200; 6]
        );
    }

    #[tokio::test]
    async fn test_middleware_blocks_after_limit() {
        use axum::{Router, middleware, routing::post};
        use tower::ServiceExt;

        if let Ok(url) = std::env::var("REDIS_URL") {
            let cfg = crate::config::CacheConfig {
                valkey_url: url,
                key_prefix: format!("rl-test-{}", uuid::Uuid::now_v7()),
                ttl: Duration::from_secs(5),
                connect_timeout: Duration::from_secs(2),
            };
            assert!(cache::init(&cfg).await.is_some());
        }
        let limiter = RateLimiter::new(2, Duration::from_secs(60), false).shared("test:login");
        let app = Router::new().route(
            "/login",
            post(|| async { "ok" })
                .route_layer(middleware::from_fn_with_state(limiter, rate_limit)),
        );
        let addr: SocketAddr = "10.0.0.1:1234".parse().unwrap();
        let mut codes = vec![];
        for _ in 0..4 {
            let mut req = Request::builder()
                .method("POST")
                .uri("/login")
                .body(Body::empty())
                .unwrap();
            req.extensions_mut().insert(ConnectInfo(addr));
            codes.push(app.clone().oneshot(req).await.unwrap().status().as_u16());
        }
        assert_eq!(codes, vec![200, 200, 429, 429]);
    }
}
