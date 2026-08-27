use crate::auth::AuthUser;
use crate::config::FrontendConfig;
use crate::database::DbPool;
use crate::dto::auth::{
    ActivityLogQuery, ChangePasswordRequest, FailedLoginsQuery, ForgotPasswordRequest,
    GithubCallbackQuery, LoginRequest, LogoutRequest, OAuthExchangeRequest, RecentActivityQuery,
    RefreshTokenRequest, RegisterRequest, ResetPasswordRequest,
};
use crate::error::AppError;
use crate::extract::{VJson, VQuery};
use crate::rate_limit::{RateLimiter, rate_limit};
use crate::response::ApiResponse;
use crate::services::{self, auth::AuthError};
use axum::{
    Json, Router,
    extract::{Query, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    middleware,
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use rand::distr::{Alphanumeric, SampleString};
use std::time::Duration;

fn user_agent(headers: &HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned)
}

fn map_auth_error(message: &'static str, err: AuthError) -> AppError {
    match err {
        AuthError::UserExists => AppError::Conflict("Email or username already exists".to_string()),
        AuthError::InvalidCredentials => {
            AppError::Unauthorized("Invalid identifier or password".to_string())
        }
        AuthError::InvalidToken => AppError::Unauthorized("Invalid or expired token".to_string()),
        AuthError::TokenExpired => AppError::Unauthorized("Token has expired".to_string()),
        AuthError::TokenUsed => AppError::BadRequest("Token has already been used".to_string()),
        AuthError::Db(err) => AppError::from(err),
        AuthError::Token(err) => AppError::InternalServerError(format!("{}: {}", message, err)),
        AuthError::Hash(err) => AppError::InternalServerError(format!("{}: {}", message, err)),
        AuthError::Request(err) => AppError::InternalServerError(format!("{}: {}", message, err)),
        AuthError::OAuth(err) => AppError::InternalServerError(format!("{}: {}", message, err)),
    }
}

pub async fn register(
    State(pool): State<DbPool>,
    VJson(req): VJson<RegisterRequest>,
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
    VJson(req): VJson<LoginRequest>,
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
    VJson(req): VJson<ForgotPasswordRequest>,
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
    VJson(req): VJson<ResetPasswordRequest>,
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
    VJson(req): VJson<RefreshTokenRequest>,
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
    VJson(req): VJson<LogoutRequest>,
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
    VJson(req): VJson<ChangePasswordRequest>,
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
) -> Result<Json<ApiResponse<services::auth::ProfileResponse>>, AppError> {
    match services::auth::get_profile(&pool, auth_user.id).await {
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
    VQuery(query): VQuery<ActivityLogQuery>,
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
    VQuery(query): VQuery<RecentActivityQuery>,
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
    VQuery(query): VQuery<FailedLoginsQuery>,
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

// ============================================================================
// GitHub OAuth
// ============================================================================

const GITHUB_OAUTH_STATE_COOKIE: &str = "github_oauth_state";
const GITHUB_OAUTH_STATE_COOKIE_PATH: &str = "/api/auth/oauth/github";
const GITHUB_OAUTH_STATE_COOKIE_MAX_AGE_SECS: u64 = 600;

fn generate_oauth_state() -> String {
    Alphanumeric.sample_string(&mut rand::rng(), 48)
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    let prefix = format!("{name}=");
    headers
        .get(axum::http::header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .map(str::trim)
        .find_map(|part| part.strip_prefix(&prefix).map(str::to_string))
}

fn state_cookie_header(state: &str, secure: bool) -> String {
    let secure_flag = if secure { "; Secure" } else { "" };
    format!(
        "{GITHUB_OAUTH_STATE_COOKIE}={state}; Path={GITHUB_OAUTH_STATE_COOKIE_PATH}; Max-Age={GITHUB_OAUTH_STATE_COOKIE_MAX_AGE_SECS}; HttpOnly; SameSite=Lax{secure_flag}"
    )
}

fn clear_state_cookie_header() -> String {
    format!(
        "{GITHUB_OAUTH_STATE_COOKIE}=; Path={GITHUB_OAUTH_STATE_COOKIE_PATH}; Max-Age=0; HttpOnly; SameSite=Lax"
    )
}

fn append_query_param(raw_url: &str, key: &str, value: &str) -> String {
    let Ok(mut url) = reqwest::Url::parse(raw_url) else {
        return raw_url.to_string();
    };
    let pairs: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| k != key)
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    url.set_query(None);
    {
        let mut query = url.query_pairs_mut();
        for (k, v) in &pairs {
            query.append_pair(k, v);
        }
        query.append_pair(key, value);
    }
    url.to_string()
}

fn oauth_redirect(url: String, set_cookie: Option<String>) -> Response {
    let mut response = Redirect::temporary(&url).into_response();
    if let Some(cookie) = set_cookie
        && let Ok(value) = HeaderValue::from_str(&cookie)
    {
        response
            .headers_mut()
            .append(axum::http::header::SET_COOKIE, value);
    }
    response
}

/// `GET /api/auth/oauth/github` — mirrors echobackend's `GithubOAuthRedirect`.
pub async fn github_oauth_redirect() -> Response {
    let state = generate_oauth_state();
    let secure = FrontendConfig::get().url.starts_with("https://");
    let auth_url = services::auth::github_oauth_url(&state);
    oauth_redirect(auth_url, Some(state_cookie_header(&state, secure)))
}

/// `GET /api/auth/oauth/github/callback` — mirrors echobackend's `GithubOAuthCallback`.
pub async fn github_oauth_callback(
    State(pool): State<DbPool>,
    headers: HeaderMap,
    Query(query): Query<GithubCallbackQuery>,
) -> Response {
    let callback_url = &FrontendConfig::get().oauth_callback_url;
    let clear_cookie = clear_state_cookie_header();

    let Some(code) = query.code.filter(|code| !code.is_empty()) else {
        return oauth_redirect(
            append_query_param(callback_url, "error", "missing_code"),
            Some(clear_cookie),
        );
    };

    let state_cookie = cookie_value(&headers, GITHUB_OAUTH_STATE_COOKIE);
    let state_valid = match (query.state.as_deref(), state_cookie.as_deref()) {
        (Some(state), Some(cookie)) if !state.is_empty() => constant_time_eq(state, cookie),
        _ => false,
    };
    if !state_valid {
        return oauth_redirect(
            append_query_param(callback_url, "error", "invalid_state"),
            Some(clear_cookie),
        );
    }

    let user_agent = user_agent(&headers);

    let github_token = match services::auth::get_github_token(&code).await {
        Ok(token) => token,
        Err(err) => {
            services::auth::log_activity(
                &pool,
                None,
                "oauth_login_failed",
                "failure",
                None,
                user_agent.clone(),
                None,
                Some(serde_json::json!({
                    "provider": "github",
                    "error": format!("{err:?}"),
                })),
            )
            .await;
            return oauth_redirect(
                append_query_param(callback_url, "error", "github_token_failed"),
                Some(clear_cookie),
            );
        }
    };

    let mut github_user = match services::auth::fetch_github_user(&github_token).await {
        Ok(user) => user,
        Err(err) => {
            services::auth::log_activity(
                &pool,
                None,
                "oauth_login_failed",
                "failure",
                None,
                user_agent.clone(),
                None,
                Some(serde_json::json!({
                    "provider": "github",
                    "error": format!("{err:?}"),
                })),
            )
            .await;
            return oauth_redirect(
                append_query_param(callback_url, "error", "github_user_failed"),
                Some(clear_cookie),
            );
        }
    };

    if github_user.email.as_deref().unwrap_or_default().is_empty()
        && let Ok(Some(email)) = services::auth::fetch_github_user_email(&github_token).await
    {
        github_user.email = Some(email);
    }

    let tokens = match services::auth::sign_in_with_github(&pool, &github_user, user_agent).await {
        Ok(tokens) => tokens,
        Err(_) => {
            return oauth_redirect(
                append_query_param(callback_url, "error", "oauth_login_failed"),
                Some(clear_cookie),
            );
        }
    };

    let exchange_code = services::auth::create_oauth_exchange_code(tokens);
    oauth_redirect(
        append_query_param(callback_url, "code", &exchange_code),
        Some(clear_cookie),
    )
}

/// `POST /api/auth/oauth/exchange` — mirrors echobackend's `ExchangeOAuthCode`.
pub async fn exchange_oauth_code(
    VJson(req): VJson<OAuthExchangeRequest>,
) -> Result<Json<ApiResponse<services::auth::AuthTokenResponse>>, AppError> {
    let response = services::auth::exchange_oauth_code(&req.code).map_err(|err| match err {
        AuthError::InvalidToken => {
            AppError::Unauthorized("Invalid or expired OAuth code".to_string())
        }
        other => map_auth_error("Failed to exchange OAuth code", other),
    })?;

    Ok(Json(ApiResponse::success_with_message(
        "OAuth code exchanged successfully",
        response,
    )))
}

pub fn routes() -> Router<DbPool> {
    // Fixed-window auth rate limits per IP, mirroring the documented
    // echobackend limits (`docs/api/auth.md`).
    let trust_proxy =
        std::panic::catch_unwind(|| crate::config::HttpConfig::get().trust_proxy).unwrap_or(false);
    let register_limiter = RateLimiter::new(5, Duration::from_secs(5 * 60), trust_proxy);
    let login_limiter = RateLimiter::new(5, Duration::from_secs(5 * 60), trust_proxy);
    let forgot_password_limiter = RateLimiter::new(3, Duration::from_secs(5 * 60), trust_proxy);
    let reset_password_limiter = RateLimiter::new(5, Duration::from_secs(5 * 60), trust_proxy);
    let refresh_limiter = RateLimiter::new(30, Duration::from_secs(60), trust_proxy);
    let oauth_exchange_limiter = RateLimiter::new(10, Duration::from_secs(60), trust_proxy);
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
        .route("/api/auth/logout", post(logout))
        .route("/api/auth/profile", get(profile))
        .route("/api/auth/password", axum::routing::patch(change_password))
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
