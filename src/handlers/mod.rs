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

use crate::{config::HttpConfig, database::DbPool, middleware as mw, rate_limit};
use axum::{Router, extract::DefaultBodyLimit, http::StatusCode, middleware};
use tower_http::{timeout::TimeoutLayer, trace::TraceLayer};

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

    // Streaming (SSE) routes stay outside the request timeout.
    let router = Router::new()
        .merge(api_routes)
        .merge(chat::routes())
        .merge(guild::stream_routes())
        .layer(DefaultBodyLimit::max(10 * 1024 * 1024))
        .layer(middleware::from_fn(mw::security_headers))
        .layer(mw::cors(http_config))
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
