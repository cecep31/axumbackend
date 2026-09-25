mod auth;
mod bookmark;
mod chat;
mod comment;
mod exchange_rate;
mod guild;
mod health;
mod holding;
mod notification;
mod post;
mod report;
mod tag;
mod user;

use crate::{config::HttpConfig, database::DbPool, rate_limit};
use axum::{
    Router,
    extract::{DefaultBodyLimit, Request},
    http::{HeaderName, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::Response,
};
use tower_http::cors::CorsLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        HeaderName::from_static("x-frame-options"),
        HeaderValue::from_static("SAMEORIGIN"),
    );
    headers.insert(
        HeaderName::from_static("x-xss-protection"),
        HeaderValue::from_static("1; mode=block"),
    );
    headers.insert(
        HeaderName::from_static("strict-transport-security"),
        HeaderValue::from_static("max-age=3600"),
    );
    headers.insert(
        HeaderName::from_static("content-security-policy"),
        HeaderValue::from_static("default-src 'self'"),
    );
    headers.insert(
        HeaderName::from_static("referrer-policy"),
        HeaderValue::from_static("strict-origin-when-cross-origin"),
    );
    response
}

pub fn create_router(limiter: Option<rate_limit::RateLimiter>) -> Router<DbPool> {
    let http_config = HttpConfig::get();

    let api_routes = Router::new()
        .merge(health::routes())
        .merge(auth::routes())
        .merge(bookmark::routes())
        .merge(comment::routes())
        .merge(exchange_rate::routes())
        .merge(guild::routes())
        .merge(holding::routes())
        .merge(notification::routes())
        .merge(post::routes())
        .merge(report::routes())
        .merge(tag::routes())
        .merge(user::routes())
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            http_config.request_timeout,
        ));

    let cors_layer = if http_config.allow_origins.contains(&"*".to_string())
        || http_config.allow_origins.is_empty()
    {
        CorsLayer::permissive()
    } else {
        let origins: Vec<_> = http_config
            .allow_origins
            .iter()
            .filter_map(|o| o.parse().ok())
            .collect();
        CorsLayer::new()
            .allow_origin(origins)
            .allow_methods(tower_http::cors::Any)
            .allow_headers(tower_http::cors::Any)
            .allow_credentials(true)
    };

    let router = Router::new()
        .merge(api_routes)
        .merge(chat::routes())
        .merge(guild::stream_routes())
        .layer(DefaultBodyLimit::max(10 * 1024 * 1024))
        .layer(middleware::from_fn(security_headers))
        .layer(cors_layer)
        .layer(TraceLayer::new_for_http());

    if let Some(limiter) = limiter {
        router.layer(middleware::from_fn_with_state(
            limiter,
            rate_limit::rate_limit,
        ))
    } else {
        router
    }
}
