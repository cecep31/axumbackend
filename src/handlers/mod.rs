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
mod tag;
mod user;

use crate::{
    config::HttpConfig,
    http::{middleware as mw, rate_limit},
    state::AppState,
};
use axum::{Router, extract::DefaultBodyLimit, http::StatusCode, middleware};
use tower_http::{
    timeout::TimeoutLayer,
    trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer},
};
use tracing::Level;

/// Builds the router. Layer order matches echobackend's `InitMiddleware`
/// (outermost first): request logging (Trace), CORS, security headers, body
/// limit, global rate limit. CORS therefore answers preflights before the
/// limiter and decorates 413/429 responses.
pub fn create_router(limiter: Option<rate_limit::TokenBucketLimiter>) -> Router<AppState> {
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
        .merge(tag::routes())
        .merge(user::routes())
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            http_config.request_timeout,
        ));

    // Streaming (SSE) routes stay outside the request timeout.
    let mut router = Router::new()
        .merge(api_routes)
        .merge(chat::routes())
        .merge(guild::stream_routes())
        .fallback(mw::route_not_found)
        .method_not_allowed_fallback(mw::method_not_allowed);

    if let Some(limiter) = limiter {
        router = router.layer(middleware::from_fn_with_state(
            limiter,
            rate_limit::global_rate_limit,
        ));
    }

    router
        .layer(DefaultBodyLimit::max(10 * 1024 * 1024))
        .layer(middleware::from_fn(mw::security_headers))
        .layer(mw::cors(http_config))
        // tower-http logs at DEBUG by default, which the default `info` filter
        // hides; log one INFO line per request (method, uri, status, latency).
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Building the full router used to panic in tower-http's CORS checks as
    /// soon as `HTTP_ALLOW_ORIGINS` listed explicit origins; this also catches
    /// conflicting fallbacks between merged routers.
    #[test]
    fn router_builds_with_explicit_origins_and_global_limiter() {
        if HttpConfig::try_get().is_none() {
            HttpConfig::init(HttpConfig {
                trust_proxy: true,
                allow_origins: vec![
                    "https://pilput.net".to_string(),
                    "https://admin.pilput.net".to_string(),
                ],
                request_timeout: std::time::Duration::from_secs(30),
            });
        }
        let limiter =
            rate_limit::TokenBucketLimiter::new(10, std::time::Duration::from_secs(180), true);
        let _router = create_router(Some(limiter));
        let _router = create_router(None);
    }
}
