use crate::auth::AuthUser;
use crate::database::DbPool;
use crate::dto::notification::{NotificationPath, NotificationQuery};
use crate::error::AppError;
use crate::extract::{VPath, VQuery};
use crate::models::notification::{MarkAllReadResponse, NotificationResponse, UnreadCountResponse};
use crate::response::ApiResponse;
use crate::services;
use axum::{
    Json, Router,
    extract::State,
    routing::{get, patch},
};

pub async fn get_notifications(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VQuery(query): VQuery<NotificationQuery>,
) -> Result<Json<ApiResponse<Vec<NotificationResponse>>>, AppError> {
    let (limit, offset) = query.resolve();
    // echobackend: `unread` filters only when it is exactly `"true"`.
    let unread = query.unread.as_deref() == Some("true");
    let (notifications, total) =
        services::notification::get_notifications(&pool, auth_user.id, unread, limit, offset)
            .await?;

    Ok(Json(ApiResponse::with_meta_message(
        "Successfully retrieved notifications",
        notifications,
        total,
        limit,
        offset,
    )))
}

pub async fn get_unread_count(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
) -> Result<Json<ApiResponse<UnreadCountResponse>>, AppError> {
    let count = services::notification::get_unread_count(&pool, auth_user.id).await?;
    Ok(Json(ApiResponse::success_with_message(
        "Successfully retrieved unread count",
        count,
    )))
}

pub async fn mark_as_read(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VPath(params): VPath<NotificationPath>,
) -> Result<Json<ApiResponse<NotificationResponse>>, AppError> {
    match services::notification::mark_as_read(&pool, params.id, auth_user.id).await? {
        Some(notification) => Ok(Json(ApiResponse::success_with_message(
            "Notification marked as read",
            notification,
        ))),
        None => Err(AppError::NotFound("Notification not found".to_string())),
    }
}

pub async fn mark_all_as_read(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
) -> Result<Json<ApiResponse<MarkAllReadResponse>>, AppError> {
    let result = services::notification::mark_all_as_read(&pool, auth_user.id).await?;
    Ok(Json(ApiResponse::success_with_message(
        "All notifications marked as read",
        result,
    )))
}

pub fn routes() -> Router<DbPool> {
    Router::new()
        .route("/api/notifications", get(get_notifications))
        .route("/api/notifications/unread-count", get(get_unread_count))
        .route("/api/notifications/read-all", patch(mark_all_as_read))
        .route("/api/notifications/{id}/read", patch(mark_as_read))
}
