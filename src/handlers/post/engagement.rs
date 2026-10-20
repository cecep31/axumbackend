use crate::dto::common::PostIdPath;
use crate::dto::post::PostPaginationQuery;
use crate::http::auth::AuthUser;
use crate::http::error::AppError;
use crate::http::extract::{VPath, VQuery};
use crate::http::response::ApiResponse;
use crate::infra::database::DbPool;
use crate::services;
use axum::{Json, extract::State};

impl From<services::post_view::PostViewError> for AppError {
    fn from(err: services::post_view::PostViewError) -> Self {
        match err {
            services::post_view::PostViewError::Db(err) => AppError::from(err),
            services::post_view::PostViewError::PostNotFound => {
                AppError::NotFound("Post not found".to_string())
            }
        }
    }
}

impl From<services::post_like::PostLikeError> for AppError {
    fn from(err: services::post_like::PostLikeError) -> Self {
        match err {
            services::post_like::PostLikeError::Db(err) => AppError::from(err),
            services::post_like::PostLikeError::PostNotFound => {
                AppError::NotFound("Post not found".to_string())
            }
            services::post_like::PostLikeError::AlreadyLiked => {
                AppError::BadRequest("You have already liked this post".to_string())
            }
            services::post_like::PostLikeError::NotLiked => {
                AppError::BadRequest("You have not liked this post".to_string())
            }
        }
    }
}

fn header_string(headers: &axum::http::HeaderMap, name: &'static str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned)
}

pub async fn record_view(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    headers: axum::http::HeaderMap,
    VPath(params): VPath<PostIdPath>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    let ip_address =
        header_string(&headers, "x-forwarded-for").or_else(|| header_string(&headers, "x-real-ip"));
    let user_agent = header_string(&headers, "user-agent");

    services::post_view::record_view(&pool, params.id, Some(auth_user.id), ip_address, user_agent)
        .await?;

    Ok(Json(ApiResponse::success_with_message(
        "View recorded successfully",
        serde_json::Value::Null,
    )))
}

pub async fn get_post_views(
    State(pool): State<DbPool>,
    _auth_user: AuthUser,
    VPath(params): VPath<PostIdPath>,
    VQuery(query): VQuery<PostPaginationQuery>,
) -> Result<Json<ApiResponse<Vec<crate::models::post_view::PostViewResponse>>>, AppError> {
    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(10);
    let (views, total) =
        services::post_view::get_views_by_post_id(&pool, params.id, limit, offset).await?;

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
    VPath(params): VPath<PostIdPath>,
) -> Result<Json<ApiResponse<crate::models::post_view::PostViewStats>>, AppError> {
    let stats = services::post_view::get_view_stats(&pool, params.id).await?;

    Ok(Json(ApiResponse::success_with_message(
        "Successfully retrieved view statistics",
        stats,
    )))
}

pub async fn check_user_viewed(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VPath(params): VPath<PostIdPath>,
) -> Result<Json<ApiResponse<crate::models::post_view::ViewStatusResponse>>, AppError> {
    let status = services::post_view::has_user_viewed_post(&pool, params.id, auth_user.id).await?;

    Ok(Json(ApiResponse::success_with_message(
        "Successfully checked view status",
        status,
    )))
}

pub async fn like_post(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VPath(params): VPath<PostIdPath>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    services::post_like::like_post(&pool, params.id, auth_user.id).await?;

    Ok(Json(ApiResponse::success_with_message(
        "Post liked successfully",
        serde_json::Value::Null,
    )))
}

pub async fn unlike_post(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VPath(params): VPath<PostIdPath>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    services::post_like::unlike_post(&pool, params.id, auth_user.id).await?;

    Ok(Json(ApiResponse::success_with_message(
        "Post unliked successfully",
        serde_json::Value::Null,
    )))
}

pub async fn get_post_likes(
    State(pool): State<DbPool>,
    VPath(params): VPath<PostIdPath>,
    VQuery(query): VQuery<PostPaginationQuery>,
) -> Result<Json<ApiResponse<crate::models::post_like::PostLikeListResponse>>, AppError> {
    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(10);
    let likes = services::post_like::get_likes_by_post_id(&pool, params.id, limit, offset).await?;

    Ok(Json(ApiResponse::success_with_message(
        "Post likes retrieved successfully",
        likes,
    )))
}

pub async fn get_post_like_stats(
    State(pool): State<DbPool>,
    VPath(params): VPath<PostIdPath>,
) -> Result<Json<ApiResponse<crate::models::post_like::PostLikeStats>>, AppError> {
    let stats = services::post_like::get_like_stats(&pool, params.id).await?;

    Ok(Json(ApiResponse::success_with_message(
        "Like stats retrieved successfully",
        stats,
    )))
}

pub async fn check_user_liked(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VPath(params): VPath<PostIdPath>,
) -> Result<Json<ApiResponse<crate::models::post_like::LikeStatusResponse>>, AppError> {
    let status = services::post_like::has_user_liked_post(&pool, params.id, auth_user.id).await?;

    Ok(Json(ApiResponse::success_with_message(
        "Like status retrieved successfully",
        status,
    )))
}
