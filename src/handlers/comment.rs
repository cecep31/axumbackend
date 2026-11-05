use crate::dto::comment::{CommentPostPath, CommentRequest};
use crate::dto::validation::parse_canonical_uuid;
use crate::http::auth::{AuthUser, is_super_admin};
use crate::http::error::AppError;
use crate::http::extract::{VBind, VPath, bind_message, raw_path_param};
use crate::http::response::ApiResponse;
use crate::infra::database::DbPool;
use crate::models::comment::CommentResponse;
use crate::services::{self, comment::CommentError};
use crate::state::AppState;
use axum::{
    Json, Router,
    extract::{FromRequestParts, State},
    http::{StatusCode, request::Parts},
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

/// echobackend's `c.Bind` failure message for the comment bodies.
const BIND_MESSAGE: &str = "Invalid request payload";

/// `/api/posts/{id}/comments/{comment_id}`, checked like echobackend's
/// `UpdateComment` / `DeleteComment`: the post id first (400 "Invalid post
/// ID"), then the comment id (400 "Invalid comment ID"), both without `error`
/// and both lowercase hyphenated UUIDs only. Declared after `AuthUser` and
/// before the body, so the ids are checked before `c.Bind`.
pub struct CommentIdsParam {
    pub post_id: Uuid,
    pub comment_id: Uuid,
}

impl<S: Send + Sync> FromRequestParts<S> for CommentIdsParam {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let post_id = raw_path_param(parts, state, "id")
            .await
            .as_deref()
            .and_then(parse_canonical_uuid)
            .ok_or_else(|| AppError::bad_request("Invalid post ID", None))?;
        let comment_id = raw_path_param(parts, state, "comment_id")
            .await
            .as_deref()
            .and_then(parse_canonical_uuid)
            .ok_or_else(|| AppError::bad_request("Invalid comment ID", None))?;
        Ok(CommentIdsParam {
            post_id,
            comment_id,
        })
    }
}

pub async fn create_comment(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VPath(params): VPath<CommentPostPath>,
    body: Result<VBind<CommentRequest>, AppError>,
) -> Result<(StatusCode, Json<ApiResponse<CommentResponse>>), AppError> {
    const MSG: &str = "Failed to create comment";
    let VBind(req) = body.map_err(bind_message(BIND_MESSAGE))?;
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
    ids: CommentIdsParam,
    body: Result<VBind<CommentRequest>, AppError>,
) -> Result<Json<ApiResponse<CommentResponse>>, AppError> {
    let VBind(req) = body.map_err(bind_message(BIND_MESSAGE))?;
    let comment = services::comment::update_comment(
        &pool,
        ids.post_id,
        ids.comment_id,
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
    ids: CommentIdsParam,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let is_admin = is_super_admin(&pool, auth_user.id).await;
    services::comment::delete_comment(&pool, ids.post_id, ids.comment_id, auth_user.id, is_admin)
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

    async fn ids_of(uri: &str) -> Result<(Uuid, Uuid), (StatusCode, serde_json::Value)> {
        let router: Router = Router::new().route(
            "/api/posts/{id}/comments/{comment_id}",
            axum::routing::get(|ids: CommentIdsParam| async move {
                format!("{} {}", ids.post_id, ids.comment_id)
            }),
        );
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
        if status == StatusCode::OK {
            let text = String::from_utf8(bytes.to_vec()).unwrap();
            let (a, b) = text.split_once(' ').unwrap();
            Ok((a.parse().unwrap(), b.parse().unwrap()))
        } else {
            Err((status, serde_json::from_slice(&bytes).unwrap()))
        }
    }

    #[tokio::test]
    async fn comment_ids_are_canonical_uuids() {
        let post = "0190a8f2-1c3e-7b4a-9d6e-2f1a3b4c5d6e";
        let comment = "0190a8f2-1c3e-7b4a-9d6e-2f1a3b4c5d6f";
        assert!(
            ids_of(&format!("/api/posts/{post}/comments/{comment}"))
                .await
                .is_ok()
        );

        let (status, body) = ids_of("/api/posts/BAD/comments/also-bad")
            .await
            .unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            body,
            serde_json::json!({"success": false, "message": "Invalid post ID"})
        );

        let upper = comment.to_uppercase();
        let (status, body) = ids_of(&format!("/api/posts/{post}/comments/{upper}"))
            .await
            .unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            body,
            serde_json::json!({"success": false, "message": "Invalid comment ID"})
        );
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
