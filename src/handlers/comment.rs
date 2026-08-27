use crate::auth::AuthUser;
use crate::database::DbPool;
use crate::dto::comment::{CommentPath, CommentRequest};
use crate::dto::common::PostIdPath;
use crate::error::AppError;
use crate::extract::{VJson, VPath};
use crate::models::comment::CommentResponse;
use crate::response::ApiResponse;
use crate::services::{self, comment::CommentError};
use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    routing::{get, put},
};

fn map_comment_error(err: CommentError) -> AppError {
    match err {
        CommentError::Db(err) => AppError::from(err),
        CommentError::PostNotFound => AppError::NotFound("Post not found".to_string()),
        CommentError::CommentNotFound => AppError::NotFound("Comment not found".to_string()),
        CommentError::NotOwner => AppError::Forbidden("You are not the comment author".to_string()),
    }
}

pub async fn create_comment(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VPath(params): VPath<PostIdPath>,
    VJson(req): VJson<CommentRequest>,
) -> Result<(StatusCode, Json<ApiResponse<CommentResponse>>), AppError> {
    let comment = services::comment::create_comment(&pool, params.id, req.text, auth_user.id)
        .await
        .map_err(map_comment_error)?;

    Ok((
        StatusCode::CREATED,
        Json(ApiResponse::success_with_message(
            "Comment created successfully",
            comment,
        )),
    ))
}

pub async fn get_comments_by_post_id(
    State(pool): State<DbPool>,
    VPath(params): VPath<PostIdPath>,
) -> Result<Json<ApiResponse<Vec<CommentResponse>>>, AppError> {
    let comments = services::comment::get_comments_by_post_id(&pool, params.id)
        .await
        .map_err(map_comment_error)?;

    Ok(Json(ApiResponse::success_with_message(
        "Comments fetched successfully",
        comments,
    )))
}

pub async fn update_comment(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VPath(params): VPath<CommentPath>,
    VJson(req): VJson<CommentRequest>,
) -> Result<Json<ApiResponse<CommentResponse>>, AppError> {
    let comment = services::comment::update_comment(
        &pool,
        params.id,
        params.comment_id,
        req.text,
        auth_user.id,
    )
    .await
    .map_err(map_comment_error)?;

    Ok(Json(ApiResponse::success_with_message(
        "Comment updated successfully",
        comment,
    )))
}

pub async fn delete_comment(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VPath(params): VPath<CommentPath>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    services::comment::delete_comment(&pool, params.id, params.comment_id, auth_user.id)
        .await
        .map_err(map_comment_error)?;

    Ok(Json(ApiResponse::success_with_message(
        "Comment deleted successfully",
        serde_json::Value::Null,
    )))
}

pub fn routes() -> Router<DbPool> {
    Router::new()
        .route(
            "/api/posts/{id}/comments",
            get(get_comments_by_post_id).post(create_comment),
        )
        .route(
            "/api/posts/{id}/comments/{comment_id}",
            put(update_comment).delete(delete_comment),
        )
}
