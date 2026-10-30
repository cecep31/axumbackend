use crate::dto::bookmark::{
    BookmarkPath, BookmarkQuery, CreateBookmarkFolderRequest, FolderIdPath, MoveBookmarkRequest,
    ToggleBookmarkRequest, UpdateBookmarkFolderRequest, UpdateBookmarkRequest,
};
use crate::http::auth::AuthUser;
use crate::http::error::AppError;
use crate::http::extract::{VJson, VJsonOrDefault, VPath, VQuery};
use crate::http::response::ApiResponse;
use crate::infra::database::DbPool;
use crate::models::bookmark::{BookmarkFolderResponse, BookmarkResponse, ToggleBookmarkResponse};
use crate::services::{self, bookmark::BookmarkError};
use crate::state::AppState;
use axum::{
    Json, Router,
    extract::State,
    routing::{get, patch, post},
};
use uuid::Uuid;

/// echobackend `handleBookmarkError`: a fixed message per error with the
/// domain text as `error` detail. Database failures get the handler's message
/// through [`bookmark_err`].
impl From<BookmarkError> for AppError {
    fn from(err: BookmarkError) -> Self {
        match err {
            BookmarkError::Db(err) => AppError::from(err),
            BookmarkError::PostNotFound => {
                AppError::not_found("Post not found", Some("post not found".to_string()))
            }
            BookmarkError::BookmarkNotFound => {
                AppError::not_found("Bookmark not found", Some("bookmark not found".to_string()))
            }
            BookmarkError::FolderNotFound => AppError::not_found(
                "Bookmark folder not found",
                Some("bookmark folder not found".to_string()),
            ),
        }
    }
}

/// Maps a service error, giving 500s echobackend's handler message
/// ("Failed to toggle bookmark", ...).
fn bookmark_err(message: &'static str) -> impl FnOnce(BookmarkError) -> AppError {
    move |err| match err {
        BookmarkError::Db(err) => AppError::from(err).context(message),
        other => other.into(),
    }
}

/// echobackend `validateBookmarkPostAndUser`: 400 with `ErrInvalidPostID`'s
/// text as message and no detail.
fn parse_post_id(raw: &str) -> Result<Uuid, AppError> {
    Uuid::parse_str(raw).map_err(|_| AppError::bad_request("invalid post ID format", None))
}

/// echobackend `validateBookmarkAndUser`: a malformed id is "not found".
fn parse_bookmark_id(raw: &str) -> Result<Uuid, AppError> {
    Uuid::parse_str(raw).map_err(|_| BookmarkError::BookmarkNotFound.into())
}

/// echobackend `validateFolderAndUser`: a malformed id is "not found".
fn parse_folder_id(raw: &str) -> Result<Uuid, AppError> {
    Uuid::parse_str(raw).map_err(|_| BookmarkError::FolderNotFound.into())
}

fn parse_folder_filter(raw: Option<String>) -> Result<Option<Option<Uuid>>, AppError> {
    match raw.as_deref() {
        None | Some("") => Ok(None),
        Some("null") => Ok(Some(None)),
        Some(value) => parse_folder_id(value).map(|id| Some(Some(id))),
    }
}

/// `POST /api/bookmarks/{id}`. The body is optional (empty = `{}`), and is
/// validated before the post id, in echobackend's order.
pub async fn toggle_bookmark(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VPath(params): VPath<BookmarkPath>,
    VJsonOrDefault(req): VJsonOrDefault<ToggleBookmarkRequest>,
) -> Result<Json<ApiResponse<ToggleBookmarkResponse>>, AppError> {
    let post_id = parse_post_id(&params.id)?;
    let folder_id = req.folder_uuid();
    let result = services::bookmark::toggle_bookmark(
        &pool,
        post_id,
        auth_user.id,
        folder_id,
        req.name,
        req.notes,
    )
    .await
    .map_err(bookmark_err("Failed to toggle bookmark"))?;

    Ok(Json(ApiResponse::success_with_message(
        "Bookmark toggled successfully",
        result,
    )))
}

pub async fn get_bookmarks(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VQuery(query): VQuery<BookmarkQuery>,
) -> Result<Json<ApiResponse<Vec<BookmarkResponse>>>, AppError> {
    let (limit, offset) = query.resolve();
    let folder_filter = parse_folder_filter(query.folder_id.clone())?;
    let (bookmarks, total) = services::bookmark::get_bookmarks_by_user(
        &pool,
        auth_user.id,
        folder_filter,
        limit,
        offset,
    )
    .await
    .map_err(bookmark_err("Failed to get bookmarks"))?;

    Ok(Json(ApiResponse::with_meta_message(
        "Bookmarks fetched successfully",
        bookmarks,
        total,
        limit,
        offset,
    )))
}

pub async fn update_bookmark(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VPath(params): VPath<BookmarkPath>,
    VJson(req): VJson<UpdateBookmarkRequest>,
) -> Result<Json<ApiResponse<BookmarkResponse>>, AppError> {
    let bookmark_id = parse_bookmark_id(&params.id)?;
    let bookmark =
        services::bookmark::update_bookmark(&pool, bookmark_id, auth_user.id, req.name, req.notes)
            .await
            .map_err(bookmark_err("Failed to update bookmark"))?;

    Ok(Json(ApiResponse::success_with_message(
        "Bookmark updated successfully",
        bookmark,
    )))
}

pub async fn move_bookmark(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VPath(params): VPath<BookmarkPath>,
    VJson(req): VJson<MoveBookmarkRequest>,
) -> Result<Json<ApiResponse<BookmarkResponse>>, AppError> {
    let bookmark_id = parse_bookmark_id(&params.id)?;
    let bookmark =
        services::bookmark::move_bookmark(&pool, bookmark_id, auth_user.id, req.folder_id)
            .await
            .map_err(bookmark_err("Failed to move bookmark"))?;

    Ok(Json(ApiResponse::success_with_message(
        "Bookmark moved successfully",
        bookmark,
    )))
}

pub async fn create_folder(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VJson(req): VJson<CreateBookmarkFolderRequest>,
) -> Result<
    (
        axum::http::StatusCode,
        Json<ApiResponse<BookmarkFolderResponse>>,
    ),
    AppError,
> {
    let folder = services::bookmark::create_folder(&pool, auth_user.id, req.name, req.description)
        .await
        .map_err(bookmark_err("Failed to create folder"))?;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(ApiResponse::success_with_message(
            "Folder created successfully",
            folder,
        )),
    ))
}

pub async fn get_folders(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
) -> Result<Json<ApiResponse<Vec<BookmarkFolderResponse>>>, AppError> {
    let folders = services::bookmark::get_folders_by_user(&pool, auth_user.id)
        .await
        .map_err(bookmark_err("Failed to get folders"))?;
    Ok(Json(ApiResponse::success_with_message(
        "Folders fetched successfully",
        folders,
    )))
}

pub async fn update_folder(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VPath(params): VPath<FolderIdPath>,
    VJson(req): VJson<UpdateBookmarkFolderRequest>,
) -> Result<Json<ApiResponse<BookmarkFolderResponse>>, AppError> {
    let folder_id = parse_folder_id(&params.folder_id)?;
    let folder = services::bookmark::update_folder(
        &pool,
        folder_id,
        auth_user.id,
        req.name,
        req.description,
    )
    .await
    .map_err(bookmark_err("Failed to update folder"))?;
    Ok(Json(ApiResponse::success_with_message(
        "Folder updated successfully",
        folder,
    )))
}

pub async fn delete_folder(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VPath(params): VPath<FolderIdPath>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    let folder_id = parse_folder_id(&params.folder_id)?;
    services::bookmark::delete_folder(&pool, folder_id, auth_user.id)
        .await
        .map_err(bookmark_err("Failed to delete folder"))?;
    Ok(Json(ApiResponse::message("Folder deleted successfully")))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/bookmarks", get(get_bookmarks))
        .route(
            "/api/bookmarks/{id}",
            post(toggle_bookmark).patch(update_bookmark),
        )
        .route("/api/bookmarks/{id}/move", patch(move_bookmark))
        .route(
            "/api/bookmarks/folders",
            post(create_folder).get(get_folders),
        )
        .route(
            "/api/bookmarks/folders/{folder_id}",
            patch(update_folder).delete(delete_folder),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{http::StatusCode, response::IntoResponse};

    async fn body_of(err: AppError) -> (StatusCode, serde_json::Value) {
        let response = err.into_response();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        (status, serde_json::from_slice(&bytes).expect("json body"))
    }

    /// echobackend `handleBookmarkError` and its id validators.
    #[tokio::test]
    async fn malformed_ids_match_echobackend() {
        let (status, body) = body_of(parse_post_id("abc").unwrap_err()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            body,
            serde_json::json!({"success": false, "message": "invalid post ID format"})
        );

        let (status, body) = body_of(parse_bookmark_id("abc").unwrap_err()).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["message"], "Bookmark not found");
        assert_eq!(body["error"], "bookmark not found");

        let (status, body) = body_of(parse_folder_id("abc").unwrap_err()).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["message"], "Bookmark folder not found");
        assert_eq!(body["error"], "bookmark folder not found");

        assert!(parse_folder_filter(Some("abc".into())).is_err());
        assert!(matches!(
            parse_folder_filter(Some("null".into())),
            Ok(Some(None))
        ));
        assert!(matches!(parse_folder_filter(None), Ok(None)));
    }

    #[tokio::test]
    async fn database_errors_carry_handler_message() {
        let (status, body) = body_of(bookmark_err("Failed to toggle bookmark")(
            BookmarkError::Db(sea_orm::DbErr::Custom("boom".into())),
        ))
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            body,
            serde_json::json!({"success": false, "message": "Failed to toggle bookmark"})
        );

        let (status, body) = body_of(bookmark_err("Failed to toggle bookmark")(
            BookmarkError::PostNotFound,
        ))
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["message"], "Post not found");
        assert_eq!(body["error"], "post not found");
    }
}
