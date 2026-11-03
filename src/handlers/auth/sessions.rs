use crate::dto::auth::{RevokeAllSessionsRequest, RevokeUserPath};
use crate::http::auth::AdminUser;
use crate::http::client_ip::RequestClient;
use crate::http::error::AppError;
use crate::http::extract::{VJson, VPath};
use crate::http::response::ApiResponse;
use crate::infra::database::DbPool;
use crate::models::auth::{RevokeAllSessionsResponse, RevokeUserSessionsResponse};
use crate::services::{self};
use axum::{Json, extract::State};

fn map_revoke_error(err: services::auth::RevokeError) -> AppError {
    match err {
        services::auth::RevokeError::UserNotFound => {
            AppError::not_found("User not found", Some("user not found".into()))
        }
        services::auth::RevokeError::Db(e) => {
            AppError::from(e).context("Failed to revoke sessions")
        }
    }
}

/// echobackend `validator.IsValidUUID`: canonical, lowercase, hyphenated.
fn is_canonical_uuid(raw: &str) -> bool {
    raw.len() == 36
        && raw.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_digit() || (b'a'..=b'f').contains(&b),
        })
}

/// `DELETE /api/auth/sessions/:userId` — admin only, mirrors echobackend's
/// `AuthHandler.RevokeUserSessions`.
pub async fn revoke_user_sessions(
    State(pool): State<DbPool>,
    admin: AdminUser,
    RequestClient(client): RequestClient,
    VPath(params): VPath<RevokeUserPath>,
) -> Result<Json<ApiResponse<RevokeUserSessionsResponse>>, AppError> {
    // Parsed here rather than by the extractor so a bad id gets echobackend's
    // "Invalid user ID format" instead of the generic path rejection.
    let target = Some(params.user_id.as_str())
        .filter(|raw| is_canonical_uuid(raw))
        .and_then(|raw| uuid::Uuid::parse_str(raw).ok())
        .ok_or_else(|| {
            AppError::bad_request(
                "Invalid user ID format",
                Some("invalid user ID format".into()),
            )
        })?;
    let revoked = services::auth::revoke_user_sessions(&pool, target, admin.0.id, client)
        .await
        .map_err(map_revoke_error)?;
    Ok(Json(ApiResponse::success_with_message(
        "Sessions revoked successfully",
        RevokeUserSessionsResponse {
            user_id: params.user_id,
            revoked_sessions: revoked,
        },
    )))
}

/// `POST /api/auth/sessions/revoke-all` — admin only, mirrors echobackend's
/// `AuthHandler.RevokeAllSessions`. Requires `{ "confirm": true }`.
pub async fn revoke_all_sessions(
    State(pool): State<DbPool>,
    admin: AdminUser,
    RequestClient(client): RequestClient,
    VJson(req): VJson<RevokeAllSessionsRequest>,
) -> Result<Json<ApiResponse<RevokeAllSessionsResponse>>, AppError> {
    if req.confirm != Some(true) {
        return Err(AppError::BadRequest(
            "Set \"confirm\": true to log out every user".to_string(),
        ));
    }
    let revoked = services::auth::revoke_all_sessions(&pool, admin.0.id, client)
        .await
        .map_err(map_revoke_error)?;
    Ok(Json(ApiResponse::success_with_message(
        "All sessions revoked successfully",
        RevokeAllSessionsResponse {
            revoked_sessions: revoked,
        },
    )))
}
