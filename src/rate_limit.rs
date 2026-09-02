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

use crate::response::ApiResponse;

#[derive(Clone)]
pub struct RateLimiter {
    inner: Arc<Mutex<HashMap<RateLimitKey, Window>>>,
    max_requests: u32,
    window: Duration,
    trust_proxy: bool,
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
            inner: Arc::new(Mutex::new(HashMap::new())),
            max_requests,
            window,
            trust_proxy,
        }
    }

    fn check(&self, key: RateLimitKey) -> Result<(), u64> {
        let now = Instant::now();
        let mut windows = self.inner.lock().expect("rate limiter lock poisoned");
        windows.retain(|_, window| now.duration_since(window.started_at) < self.window);

        let window = windows.entry(key).or_insert_with(|| Window {
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
    let key = RateLimitKey {
        path: request.uri().path().to_owned(),
        client: client_identity(&request, limiter.trust_proxy),
    };

    match limiter.check(key) {
        Ok(()) => next.run(request).await,
        Err(retry_after) => {
            let body = Json(ApiResponse::<serde_json::Value> {
                success: false,
                message: "Too many requests".to_string(),
                data: None,
                error: Some("Too many requests. Please try again later.".to_string()),
                errors: None,
                meta: None,
            });

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
        assert!(retry_after >= 1 && retry_after <= 60);
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
}
