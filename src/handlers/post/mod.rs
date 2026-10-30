mod crud;
mod engagement;
mod moderation;
mod stats;

use crate::http::auth::require_auth;
use crate::http::rate_limit::{self, RateLimiter};
use crate::state::AppState;
use axum::{
    Router,
    extract::DefaultBodyLimit,
    middleware,
    routing::{get, post},
};
use std::time::Duration;

/// Maximum post image file size. The route body limit adds multipart framing
/// slack on top (see `crate::http::upload::max_body_bytes`) so files exactly at
/// the cap still stream fully and are judged by the handler, not cut off
/// mid-stream by the limiter.
const MAX_POST_IMAGE_SIZE: u64 = 1024 * 1024;
/// Object key prefix for post images; matches echobackend.
const POST_IMAGE_PREFIX: &str = "posts/images";

pub fn routes() -> Router<AppState> {
    let trust_proxy =
        std::panic::catch_unwind(|| crate::config::HttpConfig::get().trust_proxy).unwrap_or(false);
    let report_limiter =
        RateLimiter::new(10, Duration::from_secs(3600), trust_proxy).shared("posts:report");
    // Creating posts shares the 10/hour budget of reporting: cheap enough for
    // legitimate authors, expensive enough to stop spam floods. Registered as
    // a separate same-path route so the limit applies to POST only — axum
    // merges the two `/api/posts` routes into one.
    //
    // Like echobackend (`Auth()` before the limiter), `require_auth` is the
    // outermost route layer on both limited routes (the layer added last runs
    // first), so a 401 never consumes the budget.
    let create_post_limiter =
        RateLimiter::new(10, Duration::from_secs(3600), trust_proxy).shared("posts:create");
    Router::new()
        .route("/api/posts", get(crud::get_posts))
        .route(
            "/api/posts",
            post(crud::create_post)
                .route_layer(middleware::from_fn_with_state(
                    create_post_limiter,
                    rate_limit::rate_limit,
                ))
                .route_layer(middleware::from_fn(require_auth)),
        )
        .route("/api/posts/stats", get(stats::get_post_stats))
        .route(
            "/api/posts/stats/engagement",
            get(stats::get_post_engagement),
        )
        .route("/api/posts/reports", get(moderation::list_reported_posts))
        .route("/api/posts/random", get(crud::get_random_posts))
        .route("/api/posts/trending", get(crud::get_trending_posts))
        .route("/api/posts/me", get(crud::get_my_posts))
        .route(
            "/api/posts/me/analytics",
            get(stats::get_my_posts_analytics),
        )
        .route(
            "/api/posts/me/analytics/likes-by-month",
            get(stats::get_my_posts_likes_by_month),
        )
        .route(
            "/api/posts/me/{id}",
            get(crud::get_my_post)
                .put(crud::update_my_post)
                .delete(crud::delete_my_post),
        )
        .route("/api/posts/feed/for-you", get(crud::get_posts_for_you))
        .route(
            "/api/posts/image",
            post(crud::upload_image_posts).route_layer(DefaultBodyLimit::max(
                crate::http::upload::max_body_bytes(MAX_POST_IMAGE_SIZE),
            )),
        )
        .route("/api/posts/sitemap", get(crud::get_posts_for_sitemap))
        .route(
            "/api/posts/username/{username}",
            get(crud::get_posts_by_username),
        )
        .route(
            "/api/posts/u/{username}/{slug}",
            get(crud::get_post_by_username_and_slug),
        )
        .route("/api/posts/tag/{tag}", get(crud::get_posts_by_tag))
        .route(
            "/api/posts/{id}/reports",
            post(moderation::report_post)
                .route_layer(middleware::from_fn_with_state(
                    report_limiter,
                    rate_limit::rate_limit,
                ))
                .route_layer(middleware::from_fn(require_auth))
                .get(moderation::list_post_reports),
        )
        .route(
            "/api/posts/{id}/moderation",
            post(moderation::moderate_post).get(moderation::list_moderation_actions),
        )
        .route("/api/posts/{id}/view", post(engagement::record_view))
        .route("/api/posts/{id}/views", get(engagement::get_post_views))
        .route(
            "/api/posts/{id}/view-stats",
            get(engagement::get_post_view_stats),
        )
        .route("/api/posts/{id}/viewed", get(engagement::check_user_viewed))
        .route(
            "/api/posts/{id}/like",
            post(engagement::like_post).delete(engagement::unlike_post),
        )
        .route("/api/posts/{id}/likes", get(engagement::get_post_likes))
        .route(
            "/api/posts/{id}/like-stats",
            get(engagement::get_post_like_stats),
        )
        .route("/api/posts/{id}/liked", get(engagement::check_user_liked))
        .route(
            "/api/posts/{id}",
            get(crud::get_post)
                .put(crud::update_post)
                .delete(crud::delete_post),
        )
}
