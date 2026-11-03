mod oauth;
mod sessions;

use crate::dto::auth::{
    ActivityLogQuery, ChangePasswordRequest, CheckUsernameRequest, FailedLoginsQuery,
    ForgotPasswordRequest, LoginRequest, LogoutRequest, RecentActivityQuery, RefreshTokenRequest,
    RegisterRequest, ResetPasswordRequest, UpdateProfileRequest,
};
use crate::dto::validation::parse_pagination;
use crate::http::auth::AuthUser;
use crate::http::client_ip::RequestClient;
use crate::http::error::AppError;
use crate::http::extract::{VJson, VQuery};
use crate::http::rate_limit::{RateLimiter, rate_limit};
use crate::http::response::ApiResponse;
use crate::infra::database::DbPool;
use crate::infra::email::Mailer;
use crate::models::auth::CheckUsernameResponse;
use crate::services::{self, auth::AuthError};
use crate::state::AppState;
use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    middleware,
    routing::{get, post},
};
use oauth::{exchange_oauth_code, github_oauth_callback, github_oauth_redirect};
use sessions::{revoke_all_sessions, revoke_user_sessions};
use std::sync::Arc;
use std::time::Duration;

/// echobackend `response.InternalServerError(c, message, err)`: 500 with the
/// handler message only; the cause is logged, never sent.
pub(super) fn internal_auth_error(message: &'static str, err: AuthError) -> AppError {
    match err {
        AuthError::Db(err) => AppError::from(err).context(message),
        AuthError::Token(err) => AppError::internal(message, err),
        AuthError::Hash(err) => AppError::internal(message, err),
        AuthError::Request(err) => AppError::internal(message, err),
        AuthError::OAuth(err) => AppError::internal(message, err),
        other => AppError::internal(message, format!("{other:?}")),
    }
}

pub async fn register(
    State(pool): State<DbPool>,
    RequestClient(client): RequestClient,
    VJson(req): VJson<RegisterRequest>,
) -> Result<
    (
        StatusCode,
        Json<ApiResponse<services::auth::RegisterResponse>>,
    ),
    AppError,
> {
    let user = services::auth::register(
        &pool,
        req.email,
        req.username,
        req.password,
        req.first_name,
        req.last_name,
        client,
    )
    .await
    .map_err(|err| match err {
        AuthError::UserExists => {
            AppError::conflict("Registration failed", "Email or username already exists")
        }
        other => internal_auth_error("Registration failed", other),
    })?;

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
    RequestClient(client): RequestClient,
    VJson(req): VJson<LoginRequest>,
) -> Result<Json<ApiResponse<services::auth::AuthTokenResponse>>, AppError> {
    let response = services::auth::login(&pool, &req.identifier, &req.password, client)
        .await
        .map_err(|err| match err {
            AuthError::InvalidCredentials => {
                AppError::Unauthorized("Invalid identifier or password".to_string())
            }
            other => internal_auth_error("Login failed", other),
        })?;

    Ok(Json(ApiResponse::success_with_message(
        "Login successful",
        response,
    )))
}

pub async fn forgot_password(
    State(pool): State<DbPool>,
    State(mailer): State<Arc<dyn Mailer>>,
    RequestClient(client): RequestClient,
    VJson(req): VJson<ForgotPasswordRequest>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let _ = services::auth::forgot_password(&pool, mailer.as_ref(), &req.email, client).await;
    Ok(Json(ApiResponse::message(
        "If the email exists, a password reset link has been sent",
    )))
}

pub async fn reset_password(
    State(pool): State<DbPool>,
    RequestClient(client): RequestClient,
    VJson(req): VJson<ResetPasswordRequest>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    services::auth::reset_password(&pool, &req.token, &req.password, client)
        .await
        .map_err(|err| match err {
            AuthError::InvalidToken => AppError::bad_request(
                "Invalid or expired reset token",
                Some("invalid or expired token".into()),
            ),
            AuthError::TokenUsed => AppError::bad_request(
                "Reset token has already been used",
                Some("password reset token has already been used".into()),
            ),
            AuthError::TokenExpired => AppError::bad_request(
                "Reset token has expired",
                Some("password reset token has expired".into()),
            ),
            other => internal_auth_error("Failed to reset password", other),
        })?;

    Ok(Json(ApiResponse::message("Password reset successful")))
}

pub async fn refresh_token(
    State(pool): State<DbPool>,
    RequestClient(client): RequestClient,
    VJson(req): VJson<RefreshTokenRequest>,
) -> Result<Json<ApiResponse<services::auth::AuthTokenResponse>>, AppError> {
    let response = services::auth::refresh_token(&pool, &req.refresh_token, client)
        .await
        .map_err(|err| match err {
            AuthError::InvalidToken => {
                AppError::Unauthorized("Invalid or expired refresh token".to_string())
            }
            AuthError::TokenExpired => {
                AppError::Unauthorized("Refresh token has expired".to_string())
            }
            other => internal_auth_error("Failed to refresh token", other),
        })?;

    Ok(Json(ApiResponse::success_with_message(
        "Token refreshed successfully",
        response,
    )))
}

pub async fn logout(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    RequestClient(client): RequestClient,
    VJson(req): VJson<LogoutRequest>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let _ = services::auth::logout(&pool, auth_user.id, &req.refresh_token, client).await;
    Ok(Json(ApiResponse::message("Logout successful")))
}

pub async fn change_password(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    RequestClient(client): RequestClient,
    VJson(req): VJson<ChangePasswordRequest>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    services::auth::change_password(
        &pool,
        auth_user.id,
        &req.current_password,
        &req.new_password,
        client,
    )
    .await
    .map_err(|err| match err {
        AuthError::InvalidCredentials => {
            AppError::Unauthorized("Current password is incorrect".to_string())
        }
        // echobackend: a vanished user is ErrUserNotFound, reported as a 500
        // like any other unmapped error.
        other => internal_auth_error("Failed to change password", other),
    })?;

    Ok(Json(ApiResponse::message("Password changed successfully")))
}

pub async fn profile(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
) -> Result<Json<ApiResponse<services::auth::ProfileResponse>>, AppError> {
    match services::auth::get_profile(&pool, auth_user.id).await {
        Ok(Some(user)) => Ok(Json(ApiResponse::success_with_message(
            "Profile retrieved successfully",
            user,
        ))),
        Ok(None) => Err(AppError::not_found(
            "User not found",
            Some("user not found".into()),
        )),
        Err(e) => Err(AppError::from(e).context("Failed to get profile")),
    }
}

pub async fn activity_logs(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VQuery(query): VQuery<ActivityLogQuery>,
) -> Result<Json<ApiResponse<Vec<services::auth::AuthActivityLogResponse>>>, AppError> {
    let (limit, offset) = parse_pagination(query.offset.as_deref(), query.limit.as_deref(), 20);
    let (logs, total) = services::auth::get_activity_logs(
        &pool,
        auth_user.id,
        // echobackend: an empty activity_type means no filter.
        query.activity_type.clone().filter(|t| !t.is_empty()),
        limit as u64,
        offset as u64,
    )
    .await
    .map_err(|err| internal_auth_error("Failed to get activity logs", err))?;

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
    VQuery(query): VQuery<RecentActivityQuery>,
) -> Result<Json<ApiResponse<Vec<services::auth::AuthActivityLogResponse>>>, AppError> {
    let limit = query.limit();
    let logs = services::auth::get_recent_activity(&pool, auth_user.id, limit as u64)
        .await
        .map_err(|err| internal_auth_error("Failed to get recent activity", err))?;

    Ok(Json(ApiResponse::success_with_message(
        "Recent activity retrieved successfully",
        logs,
    )))
}

pub async fn failed_logins(
    State(pool): State<DbPool>,
    _admin_user: crate::http::auth::AdminUser,
    VQuery(query): VQuery<FailedLoginsQuery>,
) -> Result<Json<ApiResponse<Vec<services::auth::AuthActivityLogResponse>>>, AppError> {
    let (limit, offset) = parse_pagination(query.offset.as_deref(), query.limit.as_deref(), 20);
    let since_hours = query.since_hours();
    let (logs, total) =
        services::auth::get_failed_logins(&pool, since_hours, limit as u64, offset as u64)
            .await
            .map_err(|err| internal_auth_error("Failed to get failed logins", err))?;

    Ok(Json(ApiResponse::with_meta_message(
        "Failed logins retrieved successfully",
        logs,
        total,
        limit,
        offset,
    )))
}

/// `POST /api/auth/check-username` — mirrors echobackend's `AuthHandler.CheckUsername`.
pub async fn check_username(
    State(pool): State<DbPool>,
    VJson(req): VJson<CheckUsernameRequest>,
) -> Result<Json<ApiResponse<CheckUsernameResponse>>, AppError> {
    let exists = services::auth::check_username_exists(&pool, &req.username)
        .await
        .map_err(|err| internal_auth_error("Failed to check username", err))?;

    Ok(Json(ApiResponse::success_with_message(
        "Username availability checked",
        CheckUsernameResponse { exists },
    )))
}

/// `PUT /api/auth/profile` — mirrors echobackend's `AuthHandler.UpdateProfile`.
pub async fn update_profile(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VJson(req): VJson<UpdateProfileRequest>,
) -> Result<Json<ApiResponse<services::auth::ProfileResponse>>, AppError> {
    let profile = services::auth::update_profile(
        &pool,
        auth_user.id,
        req.username,
        req.first_name,
        req.last_name,
    )
    .await
    .map_err(|err| match err {
        AuthError::UserExists => {
            AppError::conflict("Failed to update profile", "Username already taken")
        }
        AuthError::UserNotFound => {
            AppError::not_found("Failed to update profile", Some("user not found".into()))
        }
        other => internal_auth_error("Failed to update profile", other),
    })?;

    Ok(Json(ApiResponse::success_with_message(
        "Profile updated successfully",
        profile,
    )))
}

/// `DELETE /api/auth/account` — mirrors echobackend's `AuthHandler.DeleteAccount`.
pub async fn delete_account(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
) -> Result<Json<ApiResponse<()>>, AppError> {
    services::auth::delete_account(&pool, auth_user.id)
        .await
        // echobackend reports every failure here, a missing user included,
        // as a 500.
        .map_err(|err| internal_auth_error("Failed to delete account", err))?;

    Ok(Json(ApiResponse::message("Account deleted successfully")))
}

pub fn routes() -> Router<AppState> {
    // Fixed-window auth rate limits per IP, mirroring the documented
    // echobackend limits (`docs/api/auth.md`). Shared through Redis under the
    // same names as echobackend when it is configured.
    let trust_proxy =
        std::panic::catch_unwind(|| crate::config::HttpConfig::get().trust_proxy).unwrap_or(false);
    let register_limiter =
        RateLimiter::new(5, Duration::from_secs(5 * 60), trust_proxy).shared("auth:register");
    let login_limiter =
        RateLimiter::new(5, Duration::from_secs(5 * 60), trust_proxy).shared("auth:login");
    let forgot_password_limiter = RateLimiter::new(3, Duration::from_secs(5 * 60), trust_proxy)
        .shared("auth:forgot-password");
    let reset_password_limiter =
        RateLimiter::new(5, Duration::from_secs(5 * 60), trust_proxy).shared("auth:reset-password");
    let refresh_limiter =
        RateLimiter::new(30, Duration::from_secs(60), trust_proxy).shared("auth:refresh");
    let oauth_exchange_limiter =
        RateLimiter::new(10, Duration::from_secs(60), trust_proxy).shared("auth:oauth-exchange");
    let check_username_limiter = RateLimiter::new(20, Duration::from_secs(5 * 60), trust_proxy)
        .shared("auth:check-username");
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
                forgot_password_limiter,
                rate_limit,
            )),
        )
        .route(
            "/api/auth/reset-password",
            post(reset_password).route_layer(middleware::from_fn_with_state(
                reset_password_limiter,
                rate_limit,
            )),
        )
        .route(
            "/api/auth/check-username",
            post(check_username).route_layer(middleware::from_fn_with_state(
                check_username_limiter,
                rate_limit,
            )),
        )
        .route("/api/auth/logout", post(logout))
        .route("/api/auth/profile", get(profile).put(update_profile))
        .route("/api/auth/account", axum::routing::delete(delete_account))
        .route("/api/auth/password", axum::routing::patch(change_password))
        .route("/api/auth/sessions/revoke-all", post(revoke_all_sessions))
        .route(
            "/api/auth/sessions/{user_id}",
            axum::routing::delete(revoke_user_sessions),
        )
        .route("/api/auth/activity-logs", get(activity_logs))
        .route("/api/auth/activity-logs/recent", get(recent_activity))
        .route("/api/auth/activity-logs/failed-logins", get(failed_logins))
        .route("/api/auth/oauth/github", get(github_oauth_redirect))
        .route(
            "/api/auth/oauth/github/callback",
            get(github_oauth_callback),
        )
        .route(
            "/api/auth/oauth/exchange",
            post(exchange_oauth_code).route_layer(middleware::from_fn_with_state(
                oauth_exchange_limiter,
                rate_limit,
            )),
        )
}
