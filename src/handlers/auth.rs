use crate::auth::AuthUser;
use crate::database::DbPool;
use crate::dto::auth::{
    ActivityLogQuery, ChangePasswordRequest, FailedLoginsQuery, ForgotPasswordRequest,
    LoginRequest, LogoutRequest, RecentActivityQuery, RefreshTokenRequest, RegisterRequest,
    ResetPasswordRequest,
};
use crate::error::AppError;
use crate::rate_limit::{RateLimiter, rate_limit};
use crate::response::ApiResponse;
use crate::services::{self, auth::AuthError};
use axum::{
    Json, Router,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    middleware,
    routing::{get, post},
};
use axum_valid::Valid;
use std::time::Duration;

fn user_agent(headers: &HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned)
}

fn map_auth_error(message: &'static str, err: AuthError) -> AppError {
    match err {
        AuthError::UserExists => {
            AppError::BadRequest("Email or username already exists".to_string())
        }
        AuthError::InvalidCredentials => {
            AppError::Unauthorized("Invalid identifier or password".to_string())
        }
        AuthError::InvalidToken => AppError::Unauthorized("Invalid or expired token".to_string()),
        AuthError::TokenExpired => AppError::Unauthorized("Token has expired".to_string()),
        AuthError::TokenUsed => AppError::BadRequest("Token has already been used".to_string()),
        AuthError::Db(err) => AppError::from(err),
        AuthError::Token(err) => AppError::InternalServerError(format!("{}: {}", message, err)),
        AuthError::Hash(err) => AppError::InternalServerError(format!("{}: {}", message, err)),
    }
}

pub async fn register(
    State(pool): State<DbPool>,
    Valid(Json(req)): Valid<Json<RegisterRequest>>,
) -> Result<
    (
        StatusCode,
        Json<ApiResponse<services::auth::RegisterResponse>>,
    ),
    AppError,
> {
    let user = services::auth::register(&pool, req.email, req.username, req.password)
        .await
        .map_err(|err| map_auth_error("Registration failed", err))?;

    Ok((
        StatusCode::CREATED,
        Json(ApiResponse::success_with_message(
            "User registered successfully",
            user,
        )),
    ))
}

pub async fn login(
    State(pool): State<DbPool>,
    headers: HeaderMap,
    Valid(Json(req)): Valid<Json<LoginRequest>>,
) -> Result<Json<ApiResponse<services::auth::AuthTokenResponse>>, AppError> {
    let response =
        services::auth::login(&pool, &req.identifier, &req.password, user_agent(&headers))
            .await
            .map_err(|err| map_auth_error("Login failed", err))?;

    Ok(Json(ApiResponse::success_with_message(
        "Login successful",
        response,
    )))
}

pub async fn forgot_password(
    State(pool): State<DbPool>,
    headers: HeaderMap,
    Valid(Json(req)): Valid<Json<ForgotPasswordRequest>>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    let _ = services::auth::forgot_password(&pool, &req.email, user_agent(&headers)).await;
    Ok(Json(ApiResponse::success_with_message(
        "If the email exists, a password reset link has been sent",
        serde_json::Value::Null,
    )))
}

pub async fn reset_password(
    State(pool): State<DbPool>,
    headers: HeaderMap,
    Valid(Json(req)): Valid<Json<ResetPasswordRequest>>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    services::auth::reset_password(&pool, &req.token, &req.password, user_agent(&headers))
        .await
        .map_err(|err| match err {
            AuthError::InvalidToken | AuthError::TokenExpired => {
                AppError::BadRequest("Invalid or expired reset token".to_string())
            }
            AuthError::TokenUsed => {
                AppError::BadRequest("Reset token has already been used".to_string())
            }
            other => map_auth_error("Failed to reset password", other),
        })?;

    Ok(Json(ApiResponse::success_with_message(
        "Password reset successful",
        serde_json::Value::Null,
    )))
}

pub async fn refresh_token(
    State(pool): State<DbPool>,
    headers: HeaderMap,
    Valid(Json(req)): Valid<Json<RefreshTokenRequest>>,
) -> Result<Json<ApiResponse<services::auth::AuthTokenResponse>>, AppError> {
    let response = services::auth::refresh_token(&pool, &req.refresh_token, user_agent(&headers))
        .await
        .map_err(|err| map_auth_error("Failed to refresh token", err))?;

    Ok(Json(ApiResponse::success_with_message(
        "Token refreshed successfully",
        response,
    )))
}

pub async fn logout(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    headers: HeaderMap,
    Valid(Json(req)): Valid<Json<LogoutRequest>>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    let _ = services::auth::logout(
        &pool,
        auth_user.id,
        &req.refresh_token,
        user_agent(&headers),
    )
    .await;
    Ok(Json(ApiResponse::success_with_message(
        "Logout successful",
        serde_json::Value::Null,
    )))
}

pub async fn change_password(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    headers: HeaderMap,
    Valid(Json(req)): Valid<Json<ChangePasswordRequest>>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    services::auth::change_password(
        &pool,
        auth_user.id,
        &req.current_password,
        &req.new_password,
        user_agent(&headers),
    )
    .await
    .map_err(|err| match err {
        AuthError::InvalidCredentials => {
            AppError::Unauthorized("Current password is incorrect".to_string())
        }
        other => map_auth_error("Failed to change password", other),
    })?;

    Ok(Json(ApiResponse::success_with_message(
        "Password changed successfully",
        serde_json::Value::Null,
    )))
}

pub async fn profile(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
) -> Result<Json<ApiResponse<crate::models::user::UserResponse>>, AppError> {
    match services::user::get_by_id(&pool, auth_user.id).await {
        Ok(Some(user)) => Ok(Json(ApiResponse::success_with_message(
            "Profile retrieved successfully",
            user,
        ))),
        Ok(None) => Err(AppError::NotFound("User not found".to_string())),
        Err(e) => Err(AppError::from(e)),
    }
}

pub async fn activity_logs(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    Valid(query): Valid<Query<ActivityLogQuery>>,
) -> Result<Json<ApiResponse<Vec<services::auth::AuthActivityLogResponse>>>, AppError> {
    let limit = query.limit.unwrap_or(20);
    let offset = query.offset.unwrap_or(0);
    let (logs, total) = services::auth::get_activity_logs(
        &pool,
        auth_user.id,
        query.activity_type.clone(),
        limit as u64,
        offset as u64,
    )
    .await
    .map_err(|err| map_auth_error("Failed to get activity logs", err))?;

    Ok(Json(ApiResponse::with_meta_message(
        "Activity logs retrieved successfully",
        logs,
        total,
        limit,
        offset,
    )))
}

pub async fn recent_activity(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    Valid(query): Valid<Query<RecentActivityQuery>>,
) -> Result<Json<ApiResponse<Vec<services::auth::AuthActivityLogResponse>>>, AppError> {
    let limit = query.limit.unwrap_or(10);
    let logs = services::auth::get_recent_activity(&pool, auth_user.id, limit as u64)
        .await
        .map_err(|err| map_auth_error("Failed to get recent activity", err))?;

    Ok(Json(ApiResponse::success_with_message(
        "Recent activity retrieved successfully",
        logs,
    )))
}

pub async fn failed_logins(
    State(pool): State<DbPool>,
    _admin_user: crate::auth::AdminUser,
    Valid(query): Valid<Query<FailedLoginsQuery>>,
) -> Result<Json<ApiResponse<Vec<services::auth::AuthActivityLogResponse>>>, AppError> {
    let limit = query.limit.unwrap_or(20);
    let offset = query.offset.unwrap_or(0);
    let since_hours = query.since_hours.unwrap_or(24);
    let (logs, total) =
        services::auth::get_failed_logins(&pool, since_hours, limit as u64, offset as u64)
            .await
            .map_err(|err| map_auth_error("Failed to get failed logins", err))?;

    Ok(Json(ApiResponse::with_meta_message(
        "Failed logins retrieved successfully",
        logs,
        total,
        limit,
        offset,
    )))
}

pub fn routes() -> Router<DbPool> {
    let login_limiter = RateLimiter::new(5, Duration::from_secs(60));
    let register_limiter = RateLimiter::new(3, Duration::from_secs(60));
    let refresh_limiter = RateLimiter::new(20, Duration::from_secs(60));
    Router::new()
        .route(
            "/api/auth/register",
            post(register)
                .route_layer(middleware::from_fn_with_state(register_limiter, rate_limit)),
        )
        .route(
            "/api/auth/login",
            post(login).route_layer(middleware::from_fn_with_state(login_limiter, rate_limit)),
        )
        .route(
            "/api/auth/refresh",
            post(refresh_token)
                .route_layer(middleware::from_fn_with_state(refresh_limiter, rate_limit)),
        )
        .route(
            "/api/auth/forgot-password",
            post(forgot_password).route_layer(middleware::from_fn_with_state(
                RateLimiter::new(5, Duration::from_secs(60)),
                rate_limit,
            )),
        )
        .route("/api/auth/reset-password", post(reset_password))
        .route("/api/auth/logout", post(logout))
        .route("/api/auth/profile", get(profile))
        .route("/api/auth/password", axum::routing::patch(change_password))
        .route("/api/auth/activity-logs", get(activity_logs))
        .route("/api/auth/activity-logs/recent", get(recent_activity))
        .route("/api/auth/activity-logs/failed-logins", get(failed_logins))
}
