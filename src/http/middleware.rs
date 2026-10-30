//! Cross-cutting HTTP layers shared by every route.

use crate::config::HttpConfig;
use crate::http::error::AppError;
use axum::{
    extract::Request,
    http::{HeaderName, HeaderValue, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use tower_http::cors::{AllowHeaders, AllowOrigin, CorsLayer};

/// echobackend `middleware.SecureWithConfig` values.
const SECURITY_HEADERS: [(&str, &str); 5] = [
    ("x-xss-protection", "1; mode=block"),
    ("x-content-type-options", "nosniff"),
    ("x-frame-options", "SAMEORIGIN"),
    ("content-security-policy", "default-src 'self'"),
    ("referrer-policy", "strict-origin-when-cross-origin"),
];

/// Sent only over HTTPS, like Echo's `Secure` middleware (`HSTSMaxAge: 3600`).
const HSTS: (&str, &str) = ("strict-transport-security", "max-age=3600");

/// Echo sends HSTS when the request is TLS or `X-Forwarded-Proto: https`.
/// This server never terminates TLS itself, so only the header counts.
fn is_https(request: &Request) -> bool {
    request.uri().scheme_str() == Some("https")
        || request
            .headers()
            .get("x-forwarded-proto")
            .is_some_and(|value| value.as_bytes() == b"https")
}

/// Adds the standard security headers to every response.
pub async fn security_headers(request: Request, next: Next) -> Response {
    let https = is_https(&request);
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    for (name, value) in SECURITY_HEADERS {
        headers.insert(
            HeaderName::from_static(name),
            HeaderValue::from_static(value),
        );
    }
    if https {
        headers.insert(
            HeaderName::from_static(HSTS.0),
            HeaderValue::from_static(HSTS.1),
        );
    }
    response
}

/// Echo's default `CORSConfig` methods.
const CORS_METHODS: [Method; 6] = [
    Method::GET,
    Method::HEAD,
    Method::PUT,
    Method::PATCH,
    Method::POST,
    Method::DELETE,
];

/// Mirrors echobackend's `middleware.CORSWithConfig{AllowOrigins}`: any
/// origin when `HTTP_ALLOW_ORIGINS` is unset or `*`, otherwise only the listed
/// origins; Echo's default methods; requested headers reflected on preflight;
/// no `Access-Control-Allow-Credentials`.
pub fn cors(http_config: &HttpConfig) -> CorsLayer {
    let allow_origin = if http_config.allow_origins.is_empty()
        || http_config.allow_origins.iter().any(|o| o == "*")
    {
        AllowOrigin::any()
    } else {
        AllowOrigin::list(
            http_config
                .allow_origins
                .iter()
                .filter_map(|o| o.parse::<HeaderValue>().ok()),
        )
    };

    CorsLayer::new()
        .allow_origin(allow_origin)
        .allow_methods(CORS_METHODS)
        .allow_headers(AllowHeaders::mirror_request())
}

/// Router fallback for unknown routes: 404 envelope.
pub async fn route_not_found() -> Response {
    AppError::Detailed {
        status: StatusCode::NOT_FOUND,
        message: "Route not found".to_string(),
        error: Some("Resource not found".to_string()),
    }
    .into_response()
}

/// Router fallback for a known path with an unsupported method: 405 envelope.
pub async fn method_not_allowed() -> Response {
    AppError::Detailed {
        status: StatusCode::METHOD_NOT_ALLOWED,
        message: "Method not allowed".to_string(),
        error: Some("Method not allowed".to_string()),
    }
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, body::Body, middleware, routing::get};
    use tower::ServiceExt;

    fn app(origins: &[&str]) -> Router {
        let cfg = HttpConfig {
            trust_proxy: false,
            allow_origins: origins.iter().map(|o| o.to_string()).collect(),
            request_timeout: std::time::Duration::from_secs(30),
        };
        Router::new()
            .route("/x", get(|| async { "ok" }))
            .fallback(route_not_found)
            .method_not_allowed_fallback(method_not_allowed)
            .layer(middleware::from_fn(security_headers))
            .layer(cors(&cfg))
    }

    async fn send(app: Router, req: axum::http::Request<Body>) -> Response {
        app.oneshot(req).await.unwrap()
    }

    #[tokio::test]
    async fn explicit_origin_list_does_not_panic_and_sends_no_credentials() {
        let res = send(
            app(&["https://app.example.com", "https://admin.example.com"]),
            axum::http::Request::builder()
                .method("OPTIONS")
                .uri("/x")
                .header("origin", "https://app.example.com")
                .header("access-control-request-method", "POST")
                .header(
                    "access-control-request-headers",
                    "authorization,content-type",
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        let headers = res.headers();
        assert_eq!(
            headers["access-control-allow-origin"],
            "https://app.example.com"
        );
        assert_eq!(
            headers["access-control-allow-headers"],
            "authorization,content-type"
        );
        assert_eq!(
            headers["access-control-allow-methods"],
            "GET,HEAD,PUT,PATCH,POST,DELETE"
        );
        assert!(headers.get("access-control-allow-credentials").is_none());
    }

    #[tokio::test]
    async fn unlisted_origin_gets_no_allow_origin() {
        let res = send(
            app(&["https://app.example.com"]),
            axum::http::Request::builder()
                .uri("/x")
                .header("origin", "https://evil.example.com")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert!(res.headers().get("access-control-allow-origin").is_none());
    }

    #[tokio::test]
    async fn wildcard_origin() {
        let res = send(
            app(&["*"]),
            axum::http::Request::builder()
                .uri("/x")
                .header("origin", "https://any.example.com")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(res.headers()["access-control-allow-origin"], "*");
        assert!(
            res.headers()
                .get("access-control-allow-credentials")
                .is_none()
        );
    }

    #[tokio::test]
    async fn hsts_only_over_https() {
        let plain = send(
            app(&["*"]),
            axum::http::Request::builder()
                .uri("/x")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert!(plain.headers().get("strict-transport-security").is_none());
        assert_eq!(plain.headers()["x-frame-options"], "SAMEORIGIN");

        let https = send(
            app(&["*"]),
            axum::http::Request::builder()
                .uri("/x")
                .header("x-forwarded-proto", "https")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(https.headers()["strict-transport-security"], "max-age=3600");
    }

    #[tokio::test]
    async fn fallbacks_use_the_envelope() {
        let res = send(
            app(&["*"]),
            axum::http::Request::builder()
                .uri("/missing")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            body,
            serde_json::json!({
                "success": false,
                "message": "Route not found",
                "error": "Resource not found",
            })
        );

        let res = send(
            app(&["*"]),
            axum::http::Request::builder()
                .method("DELETE")
                .uri("/x")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);
    }
}
