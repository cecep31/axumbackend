use super::{LikePostIdParam, PostIdParam};
use crate::dto::post::PostPaginationQuery;
use crate::http::auth::{AuthUser, is_super_admin};
use crate::http::client_ip::RequestClient;
use crate::http::error::AppError;
use crate::http::extract::VQuery;
use crate::http::response::ApiResponse;
use crate::infra::database::DbPool;
use crate::services;
use axum::{Json, extract::State};

impl From<services::post_view::PostViewError> for AppError {
    fn from(err: services::post_view::PostViewError) -> Self {
        match err {
            services::post_view::PostViewError::Db(err) => AppError::from(err),
            services::post_view::PostViewError::PostNotFound(detail) => {
                AppError::NotFound(detail.to_string())
            }
            services::post_view::PostViewError::NotAuthor => {
                AppError::Forbidden("Failed to get post views".to_string())
            }
            services::post_view::PostViewError::DateRangeTooLarge => {
                AppError::BadRequest("date range must not exceed 366 days".to_string())
            }
        }
    }
}

impl From<services::post_like::PostLikeError> for AppError {
    fn from(err: services::post_like::PostLikeError) -> Self {
        match err {
            services::post_like::PostLikeError::Db(err) => AppError::from(err),
            // echobackend `response.NotFound(c, "Post not found", err)`.
            services::post_like::PostLikeError::PostNotFound(detail) => {
                AppError::not_found("Post not found", Some(detail.to_string()))
            }
            // echobackend `response.BadRequest(c, "..", nil)`: no `error`.
            services::post_like::PostLikeError::AlreadyLiked => {
                AppError::bad_request("You have already liked this post", None)
            }
            services::post_like::PostLikeError::NotLiked => {
                AppError::bad_request("You have not liked this post", None)
            }
        }
    }
}

/// `respondError(c, message, err)` for the view routes.
fn view_err(message: &'static str) -> impl FnOnce(services::post_view::PostViewError) -> AppError {
    move |err| AppError::from(err).context(message)
}

/// echobackend `response.InternalServerError(c, message, err)` for the like
/// routes: only a database failure gets the handler message.
fn like_err(message: &'static str) -> impl FnOnce(services::post_like::PostLikeError) -> AppError {
    move |err| match err {
        services::post_like::PostLikeError::Db(err) => AppError::from(err).context(message),
        other => AppError::from(other),
    }
}

pub async fn record_view(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    RequestClient(client): RequestClient,
    PostIdParam(post_id): PostIdParam,
) -> Result<Json<ApiResponse<()>>, AppError> {
    // echobackend stores `c.RealIP()` (trusted-proxy aware), never the raw
    // X-Forwarded-For header.
    services::post_view::record_view(
        &pool,
        post_id,
        Some(auth_user.id),
        client.ip_address,
        client.user_agent,
    )
    .await
    .map_err(view_err("Failed to record view"))?;

    Ok(Json(ApiResponse::message("View recorded successfully")))
}

pub async fn get_post_views(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    PostIdParam(post_id): PostIdParam,
    VQuery(query): VQuery<PostPaginationQuery>,
) -> Result<Json<ApiResponse<Vec<crate::models::post_view::PostViewResponse>>>, AppError> {
    let (limit, offset) = query.resolve(10);
    let is_admin = is_super_admin(&pool, auth_user.id).await;
    let (views, total) = services::post_view::get_views_by_post_id(
        &pool,
        post_id,
        auth_user.id,
        is_admin,
        limit,
        offset,
    )
    .await
    .map_err(view_err("Failed to get post views"))?;

    Ok(Json(ApiResponse::with_meta_message(
        "Successfully retrieved post views",
        views,
        total,
        limit,
        offset,
    )))
}

pub async fn get_post_view_stats(
    State(pool): State<DbPool>,
    PostIdParam(post_id): PostIdParam,
) -> Result<Json<ApiResponse<crate::models::post_view::PostViewStats>>, AppError> {
    let stats = services::post_view::get_view_stats(&pool, post_id)
        .await
        .map_err(view_err("Failed to get view statistics"))?;

    Ok(Json(ApiResponse::success_with_message(
        "Successfully retrieved view statistics",
        stats,
    )))
}

pub async fn check_user_viewed(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    PostIdParam(post_id): PostIdParam,
) -> Result<Json<ApiResponse<crate::models::post_view::ViewStatusResponse>>, AppError> {
    let status = services::post_view::has_user_viewed_post(&pool, post_id, auth_user.id)
        .await
        .map_err(view_err("Failed to check view status"))?;

    Ok(Json(ApiResponse::success_with_message(
        "Successfully checked view status",
        status,
    )))
}

pub async fn like_post(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    LikePostIdParam(post_id): LikePostIdParam,
) -> Result<Json<ApiResponse<()>>, AppError> {
    services::post_like::like_post(&pool, post_id, auth_user.id)
        .await
        .map_err(like_err("Failed to like post"))?;

    Ok(Json(ApiResponse::message("Post liked successfully")))
}

pub async fn unlike_post(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    LikePostIdParam(post_id): LikePostIdParam,
) -> Result<Json<ApiResponse<()>>, AppError> {
    services::post_like::unlike_post(&pool, post_id, auth_user.id)
        .await
        .map_err(like_err("Failed to unlike post"))?;

    Ok(Json(ApiResponse::message("Post unliked successfully")))
}

pub async fn get_post_likes(
    State(pool): State<DbPool>,
    LikePostIdParam(post_id): LikePostIdParam,
    VQuery(query): VQuery<PostPaginationQuery>,
) -> Result<Json<ApiResponse<crate::models::post_like::PostLikeListResponse>>, AppError> {
    let (limit, offset) = query.resolve(10);
    let likes = services::post_like::get_likes_by_post_id(&pool, post_id, limit, offset)
        .await
        .map_err(|err| AppError::from(err).context("Failed to get post likes"))?;

    Ok(Json(ApiResponse::success_with_message(
        "Post likes retrieved successfully",
        likes,
    )))
}

pub async fn get_post_like_stats(
    State(pool): State<DbPool>,
    LikePostIdParam(post_id): LikePostIdParam,
) -> Result<Json<ApiResponse<crate::models::post_like::PostLikeStats>>, AppError> {
    let stats = services::post_like::get_like_stats(&pool, post_id)
        .await
        .map_err(|err| AppError::from(err).context("Failed to get like stats"))?;

    Ok(Json(ApiResponse::success_with_message(
        "Like stats retrieved successfully",
        stats,
    )))
}

pub async fn check_user_liked(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    LikePostIdParam(post_id): LikePostIdParam,
) -> Result<Json<ApiResponse<crate::models::post_like::LikeStatusResponse>>, AppError> {
    let status = services::post_like::has_user_liked_post(&pool, post_id, auth_user.id)
        .await
        .map_err(|err| AppError::from(err).context("Failed to check like status"))?;

    Ok(Json(ApiResponse::success_with_message(
        "Like status retrieved successfully",
        status,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use services::post_like::PostLikeError;
    use services::post_view::PostViewError;

    async fn render(err: AppError) -> (StatusCode, serde_json::Value) {
        let response = err.into_response();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    #[tokio::test]
    async fn like_errors_match_echobackend() {
        let (status, body) =
            render(like_err("Failed to like post")(PostLikeError::AlreadyLiked)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            body,
            serde_json::json!({"success": false, "message": "You have already liked this post"})
        );
        let (_, body) = render(like_err("Failed to unlike post")(PostLikeError::NotLiked)).await;
        assert_eq!(
            body,
            serde_json::json!({"success": false, "message": "You have not liked this post"})
        );
        let (status, body) = render(like_err("Failed to like post")(
            PostLikeError::PostNotFound("failed to check post existence: post not found"),
        ))
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(
            body,
            serde_json::json!({
                "success": false,
                "message": "Post not found",
                "error": "failed to check post existence: post not found"
            })
        );
        let (status, body) = render(like_err("Failed to like post")(PostLikeError::Db(
            sea_orm::DbErr::Custom("boom".into()),
        )))
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            body,
            serde_json::json!({"success": false, "message": "Failed to like post"})
        );
    }

    #[tokio::test]
    async fn view_errors_match_echobackend() {
        let (status, body) = render(view_err("Failed to get view statistics")(
            PostViewError::PostNotFound("failed to verify post existence: post not found"),
        ))
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(
            body,
            serde_json::json!({
                "success": false,
                "message": "Failed to get view statistics",
                "error": "failed to verify post existence: post not found"
            })
        );
        let (status, body) = render(view_err("Failed to check view status")(
            PostViewError::PostNotFound("post not found"),
        ))
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["message"], "Failed to check view status");
        assert_eq!(body["error"], "post not found");
        let (status, body) = render(view_err("Failed to record view")(PostViewError::Db(
            sea_orm::DbErr::Custom("boom".into()),
        )))
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            body,
            serde_json::json!({"success": false, "message": "Failed to record view"})
        );
    }
}
