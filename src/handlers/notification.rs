use crate::dto::notification::NotificationQuery;
use crate::http::auth::AuthUser;
use crate::http::error::AppError;
use crate::http::extract::{VQuery, canonical_uuid_param};
use crate::http::response::ApiResponse;
use crate::infra::database::DbPool;
use crate::models::notification::{MarkAllReadResponse, NotificationResponse, UnreadCountResponse};
use crate::services;
use crate::state::AppState;
use axum::{
    Json, Router,
    extract::{FromRequestParts, State},
    http::request::Parts,
    routing::{get, patch},
};

/// The `{id}` of `PATCH /api/notifications/{id}/read`: a canonical UUID,
/// otherwise 400 "Invalid notification ID" without `error` (echobackend
/// `MsgInvalidNotificationID`), checked before any DB access.
pub struct NotificationIdParam(pub uuid::Uuid);

impl<S: Send + Sync> FromRequestParts<S> for NotificationIdParam {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        canonical_uuid_param(parts, state, "id")
            .await
            .map(NotificationIdParam)
            .ok_or_else(|| AppError::bad_request("Invalid notification ID", None))
    }
}

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
            .await
            .map_err(|err| AppError::from(err).context("Failed to get notifications"))?;

    Ok(Json(ApiResponse::with_meta_message(
        "Notifications fetched successfully",
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
    let count = services::notification::get_unread_count(&pool, auth_user.id)
        .await
        .map_err(|err| AppError::from(err).context("Failed to get unread notification count"))?;
    Ok(Json(ApiResponse::success_with_message(
        "Unread notification count fetched successfully",
        count,
    )))
}

pub async fn mark_as_read(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    NotificationIdParam(id): NotificationIdParam,
) -> Result<Json<ApiResponse<NotificationResponse>>, AppError> {
    let notification = services::notification::mark_as_read(&pool, id, auth_user.id)
        .await
        .map_err(|err| AppError::from(err).context("Failed to mark notification as read"))?;
    match notification {
        Some(notification) => Ok(Json(ApiResponse::success_with_message(
            "Notification marked as read successfully",
            notification,
        ))),
        None => Err(AppError::not_found(
            "Failed to mark notification as read",
            Some("notification not found".to_string()),
        )),
    }
}

pub async fn mark_all_as_read(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
) -> Result<Json<ApiResponse<MarkAllReadResponse>>, AppError> {
    let result = services::notification::mark_all_as_read(&pool, auth_user.id)
        .await
        .map_err(|err| AppError::from(err).context("Failed to mark all notifications as read"))?;
    Ok(Json(ApiResponse::success_with_message(
        "All notifications marked as read successfully",
        result,
    )))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/notifications", get(get_notifications))
        .route("/api/notifications/unread-count", get(get_unread_count))
        .route("/api/notifications/read-all", patch(mark_all_as_read))
        .route("/api/notifications/{id}/read", patch(mark_as_read))
}
