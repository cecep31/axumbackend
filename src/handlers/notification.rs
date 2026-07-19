use crate::auth::AuthUser;
use crate::database::DbPool;
use crate::dto::notification::{NotificationPath, NotificationQuery};
use crate::error::AppError;
use crate::models::notification::{MarkAllReadResponse, NotificationResponse, UnreadCountResponse};
use crate::response::ApiResponse;
use crate::services;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, patch},
};
use axum_valid::Valid;

pub async fn get_notifications(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    Valid(query): Valid<Query<NotificationQuery>>,
) -> Result<Json<ApiResponse<Vec<NotificationResponse>>>, AppError> {
    let limit = query.limit.unwrap_or(20);
    let offset = query.offset.unwrap_or(0);
    let (notifications, total) =
        services::notification::get_notifications(&pool, auth_user.id, query.unread, limit, offset)
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
    Valid(Path(params)): Valid<Path<NotificationPath>>,
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
