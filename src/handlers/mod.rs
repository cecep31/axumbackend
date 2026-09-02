mod auth;
mod bookmark;
mod chat;
mod comment;
mod exchange_rate;
mod health;
mod holding;
mod notification;
mod post;
mod report;
mod tag;
mod user;

use crate::{config::HttpConfig, database::DbPool, rate_limit};
use axum::{Router, middleware};
use tower_http::cors::CorsLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

pub fn create_router(limiter: Option<rate_limit::RateLimiter>) -> Router<DbPool> {
    let http_config = HttpConfig::get();

    let api_routes = Router::new()
        .merge(health::routes())
        .merge(auth::routes())
        .merge(bookmark::routes())
        .merge(comment::routes())
        .merge(exchange_rate::routes())
        .merge(holding::routes())
        .merge(notification::routes())
        .merge(post::routes())
        .merge(report::routes())
        .merge(tag::routes())
        .merge(user::routes())
        .layer(TimeoutLayer::new(http_config.request_timeout));

    let router = Router::new()
        .merge(api_routes)
        .merge(chat::routes())
        // TraceLayer should be added early to trace all requests
        // It provides good defaults: logs method, uri, status, latency automatically
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive());

    if let Some(limiter) = limiter {
        router.layer(middleware::from_fn_with_state(
            limiter,
            rate_limit::rate_limit,
        ))
    } else {
        router
    }
}
