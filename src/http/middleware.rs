//! Cross-cutting HTTP layers shared by every route.

use crate::config::HttpConfig;
use axum::{
    extract::Request,
    http::{HeaderName, HeaderValue},
    middleware::Next,
    response::Response,
};
use tower_http::cors::{Any, CorsLayer};

const SECURITY_HEADERS: [(&str, &str); 6] = [
    ("x-content-type-options", "nosniff"),
    ("x-frame-options", "SAMEORIGIN"),
    ("x-xss-protection", "1; mode=block"),
    ("strict-transport-security", "max-age=3600"),
    ("content-security-policy", "default-src 'self'"),
    ("referrer-policy", "strict-origin-when-cross-origin"),
];

/// Adds the standard security headers to every response.
pub async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    for (name, value) in SECURITY_HEADERS {
        headers.insert(
            HeaderName::from_static(name),
            HeaderValue::from_static(value),
        );
    }
    response
}

/// Permissive CORS when origins are unset or `*`; otherwise only the listed
/// origins, with credentials allowed.
pub fn cors(http_config: &HttpConfig) -> CorsLayer {
    if http_config.allow_origins.is_empty() || http_config.allow_origins.iter().any(|o| o == "*") {
        return CorsLayer::permissive();
    }

    let origins: Vec<_> = http_config
        .allow_origins
        .iter()
        .filter_map(|o| o.parse().ok())
        .collect();
    CorsLayer::new()
        .allow_origin(origins)
        .allow_methods(Any)
        .allow_headers(Any)
        .allow_credentials(true)
}
