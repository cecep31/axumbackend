//! Cross-cutting HTTP layers shared by every route.

use crate::config::HttpConfig;
use crate::http::auth::AuthUser;
use crate::http::error::AppError;
use axum::{
    extract::{ConnectInfo, FromRequestParts, Request, State},
    http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;
use tower_http::cors::{AllowHeaders, AllowOrigin, CorsLayer};

/// echobackend `middleware.SecureWithConfig` values.
const SECURITY_HEADERS: [(&str, &str); 5] = [
    ("x-xss-protection", "1; mode=block"),
    ("x-content-type-options", "nosniff"),
    ("x-frame-options", "SAMEORIGIN"),
    ("content-security-policy", "default-src 'self'"),
    ("referrer-policy", "strict-origin-when-cross-origin"),
];

/// Sent only over HTTPS, like Echo's `Secure` middleware (`HSTSMaxAge: 3600`,
/// `HSTSExcludeSubdomains: false`).
const HSTS: (&str, &str) = (
    "strict-transport-security",
    "max-age=3600; includeSubdomains",
);

/// Echo v5 `c.Scheme() == "https"` (fiberbackend `isHTTPS`): this server never
/// terminates TLS, so it is HTTPS only when the direct peer is a trusted
/// proxy (loopback, private or link-local) that forwarded an https scheme.
/// Forwarding headers from any other peer are ignored.
fn is_https(request: &Request) -> bool {
    if request.uri().scheme_str() == Some("https") {
        return true;
    }
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(addr)| addr.ip());
    peer.is_some_and(is_trusted_scheme_peer) && forwarded_scheme(request.headers()) == "https"
}

/// Go's `ip.IsLoopback() || ip.IsPrivate() || ip.IsLinkLocalUnicast()`
/// (unlike `client_ip::is_trusted_hop`, CGNAT is not included).
fn is_trusted_scheme_peer(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => ip.is_loopback() || ip.is_private() || ip.is_link_local(),
        IpAddr::V6(ip) => {
            if let Some(v4) = ip.to_ipv4_mapped() {
                return is_trusted_scheme_peer(IpAddr::V4(v4));
            }
            ip.is_loopback()
                || (ip.segments()[0] & 0xfe00) == 0xfc00
                || (ip.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

/// Echo's `schemeFromHeaders` precedence: when `X-Forwarded-Proto` is present
/// only its last value counts (added by the nearest proxy); otherwise
/// `X-Forwarded-Protocol`, `X-Forwarded-Ssl: on` and `X-Url-Scheme`, in order.
fn forwarded_scheme(headers: &HeaderMap) -> &'static str {
    let header = |name: &str| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
    };
    if let Some(last) = headers
        .get_all("x-forwarded-proto")
        .iter()
        .next_back()
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
    {
        let last = last.rsplit(',').next().unwrap_or_default().trim();
        return canonical_proto(last).unwrap_or("http");
    }
    if let Some(scheme) = canonical_proto(header("x-forwarded-protocol")) {
        return scheme;
    }
    if header("x-forwarded-ssl") == "on" {
        return "https";
    }
    canonical_proto(header("x-url-scheme")).unwrap_or("http")
}

fn canonical_proto(proto: &str) -> Option<&'static str> {
    ["http", "https", "ws", "wss"]
        .into_iter()
        .find(|p| p.eq_ignore_ascii_case(proto))
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
        AllowOrigin::list(http_config.allow_origins.iter().filter_map(|origin| {
            match origin.parse::<HeaderValue>() {
                Ok(value) => Some(value),
                Err(_) => {
                    tracing::warn!(%origin, "cors: ignoring invalid origin in HTTP_ALLOW_ORIGINS");
                    None
                }
            }
        }))
    };

    CorsLayer::new()
        .allow_origin(allow_origin)
        .allow_methods(CORS_METHODS)
        .allow_headers(AllowHeaders::mirror_request())
}

/// Path prefixes whose echobackend group carries `Auth()`. Echo v5's
/// `Group.Use` registers `RouteNotFound` for `<prefix>` and `<prefix>/*` with
/// the group middleware, so an unknown path there is a 401 without a token.
const AUTH_GROUP_PREFIXES: [&str; 6] = [
    "/api/users",
    "/api/bookmarks",
    "/api/notifications",
    "/api/holdings",
    "/api/holding-types",
    "/api/exchange-rates",
];

fn in_auth_group(path: &str) -> bool {
    AUTH_GROUP_PREFIXES.iter().any(|prefix| {
        path.strip_prefix(prefix)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    })
}

/// Router fallback for unknown routes: 404 envelope. Under an authenticated
/// echobackend group the `AuthUser` check runs first (401 without a valid
/// token).
pub async fn route_not_found(request: Request) -> Response {
    if in_auth_group(request.uri().path()) {
        let (mut parts, _) = request.into_parts();
        if let Err(rejection) = AuthUser::from_request_parts(&mut parts, &()).await {
            return rejection;
        }
    }
    AppError::Detailed {
        status: StatusCode::NOT_FOUND,
        message: "Route not found".to_string(),
        error: Some("Resource not found".to_string()),
    }
    .into_response()
}

/// Handler-level request timeout (`HTTP_REQUEST_TIMEOUT_SECS`). echobackend
/// has none (only 60 s server read/write timeouts), so an expired request is
/// rendered like its `HTTPStatus(408)`: `{message: "Request Timeout", error:
/// "Request Timeout"}`. A zero duration disables it.
pub async fn request_timeout(
    State(timeout): State<Duration>,
    request: Request,
    next: Next,
) -> Response {
    if timeout.is_zero() {
        return next.run(request).await;
    }
    match tokio::time::timeout(timeout, next.run(request)).await {
        Ok(response) => response,
        Err(_) => AppError::status_text(StatusCode::REQUEST_TIMEOUT).into_response(),
    }
}

/// `CatchPanicLayer` handler, echobackend `RecoverWithLog`: logs the panic and
/// answers 500 `{success: false, message: "Internal server error"}`.
pub fn panic_response(panic: Box<dyn std::any::Any + Send + 'static>) -> Response {
    let detail = panic
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| panic.downcast_ref::<&str>().copied())
        .unwrap_or("unknown panic");
    tracing::error!(error = %detail, "panic recovered");
    AppError::InternalServerError("Internal server error".to_string()).into_response()
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
        // Forwarded headers from an untrusted (or unknown) peer are ignored.
        assert!(https.headers().get("strict-transport-security").is_none());

        let from_peer = |peer: &str, headers: &[(&str, &str)]| {
            let mut builder = axum::http::Request::builder().uri("/x");
            for (name, value) in headers {
                builder = builder.header(*name, *value);
            }
            let mut req = builder.body(Body::empty()).unwrap();
            req.extensions_mut()
                .insert(ConnectInfo(peer.parse::<SocketAddr>().unwrap()));
            send(app(&["*"]), req)
        };
        let res = from_peer("10.0.0.2:5000", &[("x-forwarded-proto", "https")]).await;
        assert_eq!(
            res.headers()["strict-transport-security"],
            "max-age=3600; includeSubdomains"
        );
        let res = from_peer("203.0.113.9:5000", &[("x-forwarded-proto", "https")]).await;
        assert!(res.headers().get("strict-transport-security").is_none());
        // Only the last X-Forwarded-Proto value counts.
        let res = from_peer("127.0.0.1:5000", &[("x-forwarded-proto", "https, http")]).await;
        assert!(res.headers().get("strict-transport-security").is_none());
        let res = from_peer("127.0.0.1:5000", &[("x-forwarded-proto", "http, HTTPS")]).await;
        assert!(res.headers().get("strict-transport-security").is_some());
        let res = from_peer("127.0.0.1:5000", &[("x-forwarded-ssl", "on")]).await;
        assert!(res.headers().get("strict-transport-security").is_some());
        let res = from_peer("127.0.0.1:5000", &[("x-url-scheme", "https")]).await;
        assert!(res.headers().get("strict-transport-security").is_some());
    }

    #[test]
    fn forwarded_scheme_precedence() {
        let headers = |pairs: &[(&'static str, &'static str)]| {
            let mut map = HeaderMap::new();
            for (name, value) in pairs {
                map.append(*name, HeaderValue::from_static(value));
            }
            map
        };
        assert_eq!(forwarded_scheme(&headers(&[])), "http");
        assert_eq!(
            forwarded_scheme(&headers(&[
                ("x-forwarded-proto", "https"),
                ("x-forwarded-proto", "http")
            ])),
            "http"
        );
        assert_eq!(
            forwarded_scheme(&headers(&[
                ("x-forwarded-proto", "bogus"),
                ("x-forwarded-ssl", "on")
            ])),
            "http"
        );
        assert_eq!(
            forwarded_scheme(&headers(&[("x-forwarded-protocol", "https")])),
            "https"
        );
    }

    #[tokio::test]
    async fn unknown_path_under_auth_group_requires_auth() {
        assert!(in_auth_group("/api/users"));
        assert!(in_auth_group("/api/bookmarks/x/y"));
        assert!(!in_auth_group("/api/usersx"));
        assert!(!in_auth_group("/api/posts/x"));

        let res = send(
            app(&["*"]),
            axum::http::Request::builder()
                .uri("/api/bookmarks/nope/deeper")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["message"], "Missing authorization header");

        let res = send(
            app(&["*"]),
            axum::http::Request::builder()
                .uri("/api/unknown")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn request_timeout_uses_the_envelope() {
        let app = Router::new()
            .route(
                "/slow",
                get(|| async {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    "late"
                }),
            )
            .layer(middleware::from_fn_with_state(
                Duration::from_millis(10),
                request_timeout,
            ));
        let res = send(
            app,
            axum::http::Request::builder()
                .uri("/slow")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::REQUEST_TIMEOUT);
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            body,
            serde_json::json!({"success": false, "message": "Request Timeout", "error": "Request Timeout"})
        );
    }

    #[tokio::test]
    async fn panics_become_a_500_envelope_with_cors_headers() {
        let cfg = HttpConfig {
            trust_proxy: false,
            allow_origins: vec!["*".to_string()],
            request_timeout: Duration::from_secs(30),
        };
        async fn boom() -> &'static str {
            panic!("boom")
        }
        let app = Router::new()
            .route("/boom", get(boom))
            .layer(tower_http::catch_panic::CatchPanicLayer::custom(
                panic_response,
            ))
            .layer(middleware::from_fn(security_headers))
            .layer(cors(&cfg));
        let res = send(
            app,
            axum::http::Request::builder()
                .uri("/boom")
                .header("origin", "https://any.example.com")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(res.headers()["access-control-allow-origin"], "*");
        assert_eq!(res.headers()["x-frame-options"], "SAMEORIGIN");
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            body,
            serde_json::json!({"success": false, "message": "Internal server error"})
        );
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
