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
    use axum::http::Request;
    use std::net::SocketAddr;

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
}
