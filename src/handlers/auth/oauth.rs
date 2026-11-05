use super::internal_auth_error;
use crate::config::FrontendConfig;
use crate::dto::auth::{GithubCallbackQuery, OAuthExchangeRequest};
use crate::http::client_ip::RequestClient;
use crate::http::error::AppError;
use crate::http::extract::VBind;
use crate::http::response::ApiResponse;
use crate::infra::database::DbPool;
use crate::services::{self, auth::AuthError};
use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, HeaderValue},
    response::{IntoResponse, Redirect, Response},
};
use rand::distr::{Alphanumeric, SampleString};

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
    RequestClient(client): RequestClient,
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

    let github_token = match services::auth::get_github_token(&code).await {
        Ok(token) => token,
        Err(err) => {
            services::auth::log_activity(
                &pool,
                None,
                "oauth_login_failed",
                "failure",
                client.ip_address.clone(),
                client.user_agent.clone(),
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
                client.ip_address.clone(),
                client.user_agent.clone(),
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

    let tokens = match services::auth::sign_in_with_github(&pool, &github_user, client).await {
        Ok(tokens) => tokens,
        Err(_) => {
            return oauth_redirect(
                append_query_param(callback_url, "error", "oauth_login_failed"),
                Some(clear_cookie),
            );
        }
    };

    let exchange_code = services::auth::create_oauth_exchange_code(tokens).await;
    oauth_redirect(
        append_query_param(callback_url, "code", &exchange_code),
        Some(clear_cookie),
    )
}

/// `POST /api/auth/oauth/exchange` — mirrors echobackend's `ExchangeOAuthCode`.
pub async fn exchange_oauth_code(
    VBind(req): VBind<OAuthExchangeRequest>,
) -> Result<Json<ApiResponse<services::auth::AuthTokenResponse>>, AppError> {
    let response = services::auth::exchange_oauth_code(&req.code)
        .await
        .map_err(|err| match err {
            AuthError::InvalidToken => {
                AppError::Unauthorized("Invalid or expired OAuth code".to_string())
            }
            other => internal_auth_error("Failed to exchange OAuth code", other),
        })?;

    Ok(Json(ApiResponse::success_with_message(
        "OAuth code exchanged successfully",
        response,
    )))
}
