use crate::dto::comment::{CommentPath, CommentPostPath, CommentRequest};
use crate::http::auth::{AuthUser, is_super_admin};
use crate::http::error::AppError;
use crate::http::extract::{VJson, VPath};
use crate::http::response::ApiResponse;
use crate::infra::database::DbPool;
use crate::models::comment::CommentResponse;
use crate::services::{self, comment::CommentError};
use crate::state::AppState;
use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    routing::{get, put},
};
use uuid::Uuid;

/// Domain error -> HTTP, with echobackend's domain texts as the `error`
/// detail ("post not found", "comment not found"). Handlers add their own
/// `message` through [`comment_err`].
impl From<CommentError> for AppError {
    fn from(err: CommentError) -> Self {
        match err {
            CommentError::Db(err) => AppError::from(err),
            CommentError::PostNotFound => AppError::NotFound("post not found".to_string()),
            CommentError::CommentNotFound => AppError::NotFound("comment not found".to_string()),
            CommentError::NotOwner => {
                AppError::Forbidden("not authorized to modify this comment".to_string())
            }
            CommentError::SpamRejected(reason) => {
                AppError::spam_rejected("Content rejected by anti-spam policy", "text", reason)
            }
        }
    }
}

/// echobackend `respondError(c, message, err)`: `message` is the handler's,
/// `error` the domain detail (404) or "Access forbidden" (403); 500s carry
/// only `message`. The spam 422 is left as is.
fn comment_err(message: &'static str) -> impl FnOnce(CommentError) -> AppError {
    move |err| AppError::from(err).context(message)
}

/// echobackend's comment services map any `GetPostByID` failure, including a
/// malformed id, to `ErrPostNotFound`.
fn parse_post_id(raw: &str, message: &'static str) -> Result<Uuid, AppError> {
    Uuid::parse_str(raw).map_err(|_| comment_err(message)(CommentError::PostNotFound))
}

pub async fn create_comment(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VPath(params): VPath<CommentPostPath>,
    VJson(req): VJson<CommentRequest>,
) -> Result<(StatusCode, Json<ApiResponse<CommentResponse>>), AppError> {
    const MSG: &str = "Failed to create comment";
    let post_id = parse_post_id(&params.id, MSG)?;
    let comment = services::comment::create_comment(&pool, post_id, req.text, auth_user.id)
        .await
        .map_err(comment_err(MSG))?;

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
    VPath(params): VPath<CommentPostPath>,
) -> Result<Json<ApiResponse<Vec<CommentResponse>>>, AppError> {
    const MSG: &str = "Comments fetched failed";
    let post_id = parse_post_id(&params.id, MSG)?;
    let comments = services::comment::get_comments_by_post_id(&pool, post_id)
        .await
        .map_err(comment_err(MSG))?;

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
    .map_err(comment_err("Failed to update comment"))?;

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
    let is_admin = is_super_admin(&pool, auth_user.id).await;
    services::comment::delete_comment(&pool, params.id, params.comment_id, auth_user.id, is_admin)
        .await
        .map_err(comment_err("Failed to delete comment"))?;

    Ok(Json(ApiResponse::message("Comment deleted successfully")))
}

pub fn routes() -> Router<AppState> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;
    use sea_orm::DbErr;

    async fn body_of(err: AppError) -> (StatusCode, serde_json::Value) {
        let response = err.into_response();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        (status, serde_json::from_slice(&bytes).expect("json body"))
    }

    /// echobackend `respondError(c, "<handler message>", err)` envelopes.
    #[tokio::test]
    async fn comment_errors_match_echobackend() {
        let (status, body) = body_of(comment_err("Failed to create comment")(
            CommentError::PostNotFound,
        ))
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(
            body,
            serde_json::json!({"success": false, "message": "Failed to create comment", "error": "post not found"})
        );

        let (status, body) = body_of(comment_err("Failed to update comment")(
            CommentError::CommentNotFound,
        ))
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["message"], "Failed to update comment");
        assert_eq!(body["error"], "comment not found");

        let (status, body) = body_of(comment_err("Failed to delete comment")(
            CommentError::NotOwner,
        ))
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["message"], "Failed to delete comment");
        assert_eq!(body["error"], "Access forbidden");

        let (status, body) = body_of(comment_err("Comments fetched failed")(CommentError::Db(
            DbErr::Custom("boom".into()),
        )))
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            body,
            serde_json::json!({"success": false, "message": "Comments fetched failed"})
        );

        let (status, body) = body_of(comment_err("Failed to create comment")(
            CommentError::SpamRejected("spam".into()),
        ))
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["message"], "Validation failed");
        assert_eq!(body["error"], "Content rejected by anti-spam policy");
    }

    #[tokio::test]
    async fn malformed_post_id_is_post_not_found() {
        let (status, body) =
            body_of(parse_post_id("not-a-uuid", "Comments fetched failed").unwrap_err()).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["message"], "Comments fetched failed");
        assert_eq!(body["error"], "post not found");
    }
}
