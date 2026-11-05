mod crud;
mod engagement;
mod moderation;
mod stats;

use crate::dto::validation::parse_canonical_uuid;
use crate::http::auth::require_auth;
use crate::http::error::AppError;
use crate::http::extract::raw_path_param;
use crate::http::rate_limit::{self, RateLimiter};
use crate::state::AppState;
use axum::{
    Router,
    extract::{DefaultBodyLimit, FromRequestParts},
    http::request::Parts,
    middleware,
    routing::{get, post},
};
use std::time::Duration;
use uuid::Uuid;

/// The `{id}` post id, checked like echobackend's
/// `if !validator.IsValidUUID(c.Param("id"))`: anything but a lowercase
/// hyphenated UUID is 400 `{message: "Invalid post ID"}` (no `error`).
/// Declare it after the auth extractor and before any body extractor, like
/// echobackend (auth middleware first, the id checked before `c.Bind`).
pub(crate) struct PostIdParam(pub Uuid);

impl<S: Send + Sync> FromRequestParts<S> for PostIdParam {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        raw_path_param(parts, state, "id")
            .await
            .as_deref()
            .and_then(parse_canonical_uuid)
            .map(PostIdParam)
            .ok_or_else(|| AppError::bad_request("Invalid post ID", None))
    }
}

/// The `{id}` of the like routes: echobackend's `postLikeService` rejects a
/// non-canonical id with `ErrInvalidPostID`, answered as 400 `{message:
/// "invalid post ID format"}` (no `error`).
pub(crate) struct LikePostIdParam(pub Uuid);

impl<S: Send + Sync> FromRequestParts<S> for LikePostIdParam {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        raw_path_param(parts, state, "id")
            .await
            .as_deref()
            .and_then(parse_canonical_uuid)
            .map(LikePostIdParam)
            .ok_or_else(|| AppError::bad_request("invalid post ID format", None))
    }
}

/// Maximum post image request body (echobackend `BodyLimit(1MB)`), which also
/// caps the file itself.
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
            // echobackend `middleware.BodyLimit(1MB)` on the whole body.
            post(crud::upload_image_posts)
                .route_layer(DefaultBodyLimit::max(MAX_POST_IMAGE_SIZE as usize)),
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    async fn get(router: Router, uri: &str) -> (StatusCode, String) {
        let response = tower::ServiceExt::oneshot(
            router,
            axum::http::Request::builder()
                .uri(uri)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8(bytes.to_vec()).unwrap())
    }

    #[tokio::test]
    async fn post_id_params_use_echobackend_messages() {
        let router = || {
            Router::new()
                .route(
                    "/p/{id}",
                    axum::routing::get(
                        |PostIdParam(id): PostIdParam| async move { id.to_string() },
                    ),
                )
                .route(
                    "/l/{id}",
                    axum::routing::get(|LikePostIdParam(id): LikePostIdParam| async move {
                        id.to_string()
                    }),
                )
        };
        let id = "0190a8f2-1c3e-7b4a-9d6e-2f1a3b4c5d6e";
        assert_eq!(
            get(router(), &format!("/p/{id}")).await,
            (StatusCode::OK, id.into())
        );
        assert_eq!(
            get(router(), &format!("/l/{id}")).await,
            (StatusCode::OK, id.into())
        );

        let upper = id.to_uppercase();
        let simple = id.replace('-', "");
        for bad in ["not-a-uuid", upper.as_str(), simple.as_str()] {
            let (status, body) = get(router(), &format!("/p/{bad}")).await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&body).unwrap(),
                serde_json::json!({"success": false, "message": "Invalid post ID"})
            );
            let (status, body) = get(router(), &format!("/l/{bad}")).await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&body).unwrap(),
                serde_json::json!({"success": false, "message": "invalid post ID format"})
            );
        }
    }
}
