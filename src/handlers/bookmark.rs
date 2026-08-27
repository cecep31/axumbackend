use crate::auth::AuthUser;
use crate::database::DbPool;
use crate::dto::bookmark::{
    BookmarkPath, BookmarkQuery, CreateBookmarkFolderRequest, FolderIdPath, MoveBookmarkRequest,
    ToggleBookmarkRequest, UpdateBookmarkFolderRequest, UpdateBookmarkRequest,
};
use crate::error::AppError;
use crate::extract::{VJson, VPath, VQuery};
use crate::models::bookmark::{BookmarkFolderResponse, BookmarkResponse, ToggleBookmarkResponse};
use crate::response::ApiResponse;
use crate::services::{self, bookmark::BookmarkError};
use axum::{
    Json, Router,
    extract::State,
    routing::{get, patch, post},
};
use uuid::Uuid;

fn map_bookmark_error(err: BookmarkError) -> AppError {
    match err {
        BookmarkError::Db(err) => AppError::from(err),
        BookmarkError::PostNotFound => AppError::NotFound("Post not found".to_string()),
        BookmarkError::BookmarkNotFound => AppError::NotFound("Bookmark not found".to_string()),
        BookmarkError::FolderNotFound => {
            AppError::NotFound("Bookmark folder not found".to_string())
        }
    }
}

fn parse_folder_filter(raw: Option<String>) -> Result<Option<Option<Uuid>>, AppError> {
    match raw.as_deref() {
        None | Some("") => Ok(None),
        Some("null") => Ok(Some(None)),
        Some(value) => Uuid::parse_str(value)
            .map(|id| Some(Some(id)))
            .map_err(|_| AppError::NotFound("Bookmark folder not found".to_string())),
    }
}

pub async fn toggle_bookmark(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VPath(params): VPath<BookmarkPath>,
    VJson(req): VJson<ToggleBookmarkRequest>,
) -> Result<Json<ApiResponse<ToggleBookmarkResponse>>, AppError> {
    let result = services::bookmark::toggle_bookmark(
        &pool,
        params.id,
        auth_user.id,
        req.folder_id,
        req.name,
        req.notes,
    )
    .await
    .map_err(map_bookmark_error)?;

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
    .map_err(map_bookmark_error)?;

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
    let bookmark =
        services::bookmark::update_bookmark(&pool, params.id, auth_user.id, req.name, req.notes)
            .await
            .map_err(map_bookmark_error)?;

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
    let bookmark = services::bookmark::move_bookmark(&pool, params.id, auth_user.id, req.folder_id)
        .await
        .map_err(map_bookmark_error)?;

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
        .map_err(map_bookmark_error)?;
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
        .map_err(map_bookmark_error)?;
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
    let folder = services::bookmark::update_folder(
        &pool,
        params.folder_id,
        auth_user.id,
        req.name,
        req.description,
    )
    .await
    .map_err(map_bookmark_error)?;
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
    services::bookmark::delete_folder(&pool, params.folder_id, auth_user.id)
        .await
        .map_err(map_bookmark_error)?;
    Ok(Json(ApiResponse::success_with_message(
        "Folder deleted successfully",
        serde_json::Value::Null,
    )))
}

pub fn routes() -> Router<DbPool> {
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
