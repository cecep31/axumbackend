use crate::auth::Claims;
use crate::config::{GitHubConfig, JwtConfig};
use crate::email;
use crate::entities::{auth_activity_logs, password_reset_tokens, sessions, users};
use bcrypt::{DEFAULT_COST, hash, verify};
use chrono::{Duration, Utc};
use jsonwebtoken::{EncodingKey, Header, encode};
use rand::{Rng, distributions::Alphanumeric};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, PaginatorTrait,
    QueryFilter, QueryOrder, QuerySelect, Set,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use uuid::Uuid;

#[derive(Serialize)]
pub struct UserBrief {
    pub id: Uuid,
    pub email: String,
    pub username: Option<String>,
}

#[derive(Serialize)]
pub struct AuthTokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub user: UserBrief,
}

#[derive(Serialize)]
pub struct RegisterResponse {
    pub id: Uuid,
    pub email: String,
    pub username: Option<String>,
}

#[derive(Serialize)]
pub struct AuthActivityLogResponse {
    pub id: Uuid,
    pub user_id: Option<Uuid>,
    pub activity_type: String,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub status: String,
    pub error_message: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub created_at: chrono::DateTime<Utc>,
}

/// Flat profile subset returned by `GET /api/auth/profile`, mirroring
/// echobackend's `AuthHandler.GetProfile` map shape (no `name`, no nested
/// `profile`, no timestamps).
#[derive(Serialize)]
pub struct ProfileResponse {
    pub id: Uuid,
    pub email: String,
    pub username: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub image: Option<String>,
    pub is_super_admin: Option<bool>,
    pub followers_count: i64,
    pub following_count: i64,
}

#[derive(Debug)]
pub enum AuthError {
    Db(DbErr),
    UserExists,
    InvalidCredentials,
    InvalidToken,
    TokenExpired,
    TokenUsed,
    Token(jsonwebtoken::errors::Error),
    Hash(bcrypt::BcryptError),
    Request(reqwest::Error),
    OAuth(String),
}

impl From<DbErr> for AuthError {
    fn from(err: DbErr) -> Self {
        Self::Db(err)
    }
}

impl From<reqwest::Error> for AuthError {
    fn from(err: reqwest::Error) -> Self {
        Self::Request(err)
    }
}

impl From<jsonwebtoken::errors::Error> for AuthError {
    fn from(err: jsonwebtoken::errors::Error) -> Self {
        Self::Token(err)
    }
}

impl From<bcrypt::BcryptError> for AuthError {
    fn from(err: bcrypt::BcryptError) -> Self {
        Self::Hash(err)
    }
}

fn generate_refresh_token() -> String {
    let random: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(86)
        .map(char::from)
        .collect();
    format!("pl_{}", random)
}

fn generate_prefixed_token(prefix: &str) -> String {
    let random: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(64)
        .map(char::from)
        .collect();
    format!("{}_{}", prefix, random)
}

fn make_user_brief(user: &users::Model) -> UserBrief {
    UserBrief {
        id: user.id,
        email: user.email.clone(),
        username: user.username.clone(),
    }
}

fn activity_response(log: auth_activity_logs::Model) -> AuthActivityLogResponse {
    AuthActivityLogResponse {
        id: log.id,
        user_id: log.user_id,
        activity_type: log.activity_type,
        ip_address: log.ip_address,
        user_agent: log.user_agent,
        status: log.status,
        error_message: log.error_message,
        metadata: log.metadata,
        created_at: log.created_at.with_timezone(&Utc),
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn log_activity(
    db: &DatabaseConnection,
    user_id: Option<Uuid>,
    activity_type: &str,
    status: &str,
    ip_address: Option<String>,
    user_agent: Option<String>,
    error_message: Option<String>,
    metadata: Option<serde_json::Value>,
) {
    let log = auth_activity_logs::ActiveModel {
        id: Set(Uuid::new_v4()),
        user_id: Set(user_id),
        activity_type: Set(activity_type.to_string()),
        ip_address: Set(ip_address),
        user_agent: Set(user_agent),
        status: Set(status.to_string()),
        error_message: Set(error_message),
        metadata: Set(metadata),
        created_at: Set(Utc::now().into()),
    };

    if let Err(err) = log.insert(db).await {
        tracing::warn!(?err, activity_type, "failed to log auth activity");
    }
}

/// Returns the flat profile subset for `GET /api/auth/profile`
/// (echobackend `AuthHandler.GetProfile`).
pub async fn get_profile(
    db: &DatabaseConnection,
    user_id: Uuid,
) -> Result<Option<ProfileResponse>, DbErr> {
    let user = users::Entity::find_by_id(user_id)
        .filter(users::Column::DeletedAt.is_null())
        .one(db)
        .await?;

    Ok(user.map(|user| ProfileResponse {
        id: user.id,
        email: user.email,
        username: user.username,
        first_name: user.first_name,
        last_name: user.last_name,
        image: user.image,
        is_super_admin: user.is_super_admin,
        followers_count: user.followers_count.unwrap_or_default(),
        following_count: user.following_count.unwrap_or_default(),
    }))
}

async fn create_token_and_session(
    db: &DatabaseConnection,
    user: &users::Model,
    user_agent: Option<String>,
) -> Result<AuthTokenResponse, AuthError> {
    let now = Utc::now();
    let jwt = JwtConfig::get();
    let exp = now + Duration::hours(jwt.expiry_hours);
    let claims = Claims {
        user_id: user.id,
        username: user.username.clone(),
        email: user.email.clone(),
        is_super_admin: user.is_super_admin,
        iat: now.timestamp() as usize,
        exp: exp.timestamp() as usize,
    };

    let access_token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(jwt.secret.as_bytes()),
    )?;

    let refresh_token = generate_refresh_token();
    let session = sessions::ActiveModel {
        refresh_token: Set(refresh_token.clone()),
        user_id: Set(user.id),
        created_at: Set(Some(now.into())),
        user_agent: Set(user_agent),
        expires_at: Set(Some(
            (now + Duration::days(jwt.refresh_token_expiry_days)).into(),
        )),
    };
    session.insert(db).await?;

    Ok(AuthTokenResponse {
        access_token,
        refresh_token,
        user: make_user_brief(user),
    })
}

pub async fn register(
    db: &DatabaseConnection,
    email: String,
    username: String,
    password: String,
) -> Result<RegisterResponse, AuthError> {
    let existing = users::Entity::find()
        .filter(
            users::Column::Email
                .eq(email.clone())
                .or(users::Column::Username.eq(username.clone())),
        )
        .one(db)
        .await?;

    if existing.is_some() {
        return Err(AuthError::UserExists);
    }

    let hashed = hash(password, DEFAULT_COST)?;
    let user = users::ActiveModel {
        id: Set(Uuid::new_v4()),
        email: Set(email),
        username: Set(Some(username)),
        password: Set(Some(hashed)),
        created_at: Set(Some(Utc::now().into())),
        updated_at: Set(Some(Utc::now().into())),
        ..Default::default()
    }
    .insert(db)
    .await?;

    log_activity(
        db,
        Some(user.id),
        "register",
        "success",
        None,
        None,
        None,
        None,
    )
    .await;

    Ok(RegisterResponse {
        id: user.id,
        email: user.email,
        username: user.username,
    })
}

pub async fn login(
    db: &DatabaseConnection,
    identifier: &str,
    password: &str,
    user_agent: Option<String>,
) -> Result<AuthTokenResponse, AuthError> {
    let user = users::Entity::find()
        .filter(
            users::Column::Email
                .eq(identifier)
                .or(users::Column::Username.eq(identifier)),
        )
        .filter(users::Column::DeletedAt.is_null())
        .one(db)
        .await?;

    let Some(user) = user else {
        log_activity(
            db,
            None,
            "login_failed",
            "failure",
            None,
            user_agent.clone(),
            None,
            None,
        )
        .await;
        return Err(AuthError::InvalidCredentials);
    };

    let Some(hashed_password) = &user.password else {
        log_activity(
            db,
            Some(user.id),
            "login_failed",
            "failure",
            None,
            user_agent.clone(),
            None,
            None,
        )
        .await;
        return Err(AuthError::InvalidCredentials);
    };

    if !verify(password, hashed_password)? {
        log_activity(
            db,
            Some(user.id),
            "login_failed",
            "failure",
            None,
            user_agent.clone(),
            None,
            None,
        )
        .await;
        return Err(AuthError::InvalidCredentials);
    }

    let response = create_token_and_session(db, &user, user_agent.clone()).await?;
    let mut active: users::ActiveModel = user.clone().into();
    active.last_logged_at = Set(Some(Utc::now().into()));
    let _ = active.update(db).await;
    log_activity(
        db,
        Some(user.id),
        "login",
        "success",
        None,
        user_agent,
        None,
        None,
    )
    .await;
    Ok(response)
}

pub async fn check_username_availability(
    db: &DatabaseConnection,
    username: &str,
) -> Result<bool, AuthError> {
    let exists = users::Entity::find()
        .filter(users::Column::Username.eq(username))
        .filter(users::Column::DeletedAt.is_null())
        .one(db)
        .await?
        .is_some();

    Ok(!exists)
}

pub async fn check_email_availability(
    db: &DatabaseConnection,
    email: &str,
) -> Result<bool, AuthError> {
    let exists = users::Entity::find()
        .filter(users::Column::Email.eq(email))
        .filter(users::Column::DeletedAt.is_null())
        .one(db)
        .await?
        .is_some();

    Ok(!exists)
}

pub async fn refresh_token(
    db: &DatabaseConnection,
    refresh_token: &str,
    user_agent: Option<String>,
) -> Result<AuthTokenResponse, AuthError> {
    let session = sessions::Entity::find_by_id(refresh_token.to_string())
        .one(db)
        .await?
        .ok_or(AuthError::InvalidToken)?;

    if let Some(expires_at) = session.expires_at
        && expires_at.with_timezone(&Utc) < Utc::now()
    {
        let _ = sessions::Entity::delete_by_id(refresh_token.to_string())
            .exec(db)
            .await;
        return Err(AuthError::TokenExpired);
    }

    let user = users::Entity::find_by_id(session.user_id)
        .filter(users::Column::DeletedAt.is_null())
        .one(db)
        .await?
        .ok_or(AuthError::InvalidToken)?;

    sessions::Entity::delete_by_id(refresh_token.to_string())
        .exec(db)
        .await?;
    let response = create_token_and_session(db, &user, user_agent.clone()).await?;
    log_activity(
        db,
        Some(user.id),
        "token_refresh",
        "success",
        None,
        user_agent,
        None,
        None,
    )
    .await;
    Ok(response)
}

pub async fn logout(
    db: &DatabaseConnection,
    user_id: Uuid,
    refresh_token: &str,
    user_agent: Option<String>,
) -> Result<(), AuthError> {
    sessions::Entity::delete_by_id(refresh_token.to_string())
        .exec(db)
        .await?;
    log_activity(
        db,
        Some(user_id),
        "logout",
        "success",
        None,
        user_agent,
        None,
        None,
    )
    .await;
    Ok(())
}

pub async fn forgot_password(
    db: &DatabaseConnection,
    email: &str,
    user_agent: Option<String>,
) -> Result<(), AuthError> {
    let user = users::Entity::find()
        .filter(users::Column::Email.eq(email))
        .filter(users::Column::DeletedAt.is_null())
        .one(db)
        .await?;

    let Some(user) = user else {
        return Ok(());
    };

    password_reset_tokens::Entity::delete_many()
        .filter(password_reset_tokens::Column::UserId.eq(user.id))
        .exec(db)
        .await?;

    let reset_token = generate_prefixed_token("pr");
    let token = password_reset_tokens::ActiveModel {
        id: Set(Uuid::new_v4()),
        user_id: Set(user.id),
        token: Set(reset_token.clone()),
        created_at: Set(Utc::now().into()),
        expires_at: Set((Utc::now() + Duration::hours(1)).into()),
        used_at: Set(None),
    };
    token.insert(db).await?;

    let reset_link = build_password_reset_link(&reset_token);

    if email::is_configured() {
        if let Err(err) = email::send_password_reset_email(email, &reset_link).await {
            log_activity(
                db,
                Some(user.id),
                "password_reset_request",
                "failure",
                None,
                user_agent,
                Some("Failed to send email".to_string()),
                None,
            )
            .await;
            tracing::warn!(?err, user_id = %user.id, "failed to send password reset email");
            return Ok(());
        }

        log_activity(
            db,
            Some(user.id),
            "password_reset_request",
            "success",
            None,
            user_agent,
            None,
            None,
        )
        .await;
        return Ok(());
    }

    log_activity(
        db,
        Some(user.id),
        "password_reset_request",
        "success",
        None,
        user_agent,
        None,
        Some(serde_json::json!({
            "devMode": true,
            "resetLink": reset_link,
        })),
    )
    .await;
    Ok(())
}

fn build_password_reset_link(token: &str) -> String {
    let base = &crate::config::EmailConfig::get().frontend_reset_password_url;
    let separator = if base.contains('?') { '&' } else { '?' };
    format!("{base}{separator}token={token}")
}

pub async fn reset_password(
    db: &DatabaseConnection,
    token: &str,
    password: &str,
    user_agent: Option<String>,
) -> Result<(), AuthError> {
    let reset_token = password_reset_tokens::Entity::find()
        .filter(password_reset_tokens::Column::Token.eq(token))
        .one(db)
        .await?
        .ok_or(AuthError::InvalidToken)?;

    if reset_token.used_at.is_some() {
        return Err(AuthError::TokenUsed);
    }

    if reset_token.expires_at.with_timezone(&Utc) < Utc::now() {
        return Err(AuthError::TokenExpired);
    }

    let user = users::Entity::find_by_id(reset_token.user_id)
        .filter(users::Column::DeletedAt.is_null())
        .one(db)
        .await?
        .ok_or(AuthError::InvalidToken)?;

    let hashed = hash(password, DEFAULT_COST)?;
    let mut active_user: users::ActiveModel = user.clone().into();
    active_user.password = Set(Some(hashed));
    active_user.updated_at = Set(Some(Utc::now().into()));
    active_user.update(db).await?;

    let mut active_token: password_reset_tokens::ActiveModel = reset_token.into();
    active_token.used_at = Set(Some(Utc::now().into()));
    active_token.update(db).await?;

    sessions::Entity::delete_many()
        .filter(sessions::Column::UserId.eq(user.id))
        .exec(db)
        .await?;

    log_activity(
        db,
        Some(user.id),
        "password_reset",
        "success",
        None,
        user_agent,
        None,
        None,
    )
    .await;
    Ok(())
}

pub async fn change_password(
    db: &DatabaseConnection,
    user_id: Uuid,
    current_password: &str,
    new_password: &str,
    user_agent: Option<String>,
) -> Result<(), AuthError> {
    let user = users::Entity::find_by_id(user_id)
        .filter(users::Column::DeletedAt.is_null())
        .one(db)
        .await?
        .ok_or(AuthError::InvalidCredentials)?;

    let Some(hashed_password) = &user.password else {
        return Err(AuthError::InvalidCredentials);
    };

    if !verify(current_password, hashed_password)? {
        log_activity(
            db,
            Some(user.id),
            "password_change",
            "failure",
            None,
            user_agent,
            None,
            None,
        )
        .await;
        return Err(AuthError::InvalidCredentials);
    }

    let hashed = hash(new_password, DEFAULT_COST)?;
    let mut active_user: users::ActiveModel = user.clone().into();
    active_user.password = Set(Some(hashed));
    active_user.updated_at = Set(Some(Utc::now().into()));
    active_user.update(db).await?;

    log_activity(
        db,
        Some(user.id),
        "password_change",
        "success",
        None,
        user_agent,
        None,
        None,
    )
    .await;
    Ok(())
}

pub async fn get_activity_logs(
    db: &DatabaseConnection,
    user_id: Uuid,
    activity_type: Option<String>,
    limit: u64,
    offset: u64,
) -> Result<(Vec<AuthActivityLogResponse>, i64), AuthError> {
    let mut query =
        auth_activity_logs::Entity::find().filter(auth_activity_logs::Column::UserId.eq(user_id));

    if let Some(activity_type) = activity_type {
        query = query.filter(auth_activity_logs::Column::ActivityType.eq(activity_type));
    }

    let total = query.clone().count(db).await? as i64;
    let logs = query
        .order_by_desc(auth_activity_logs::Column::CreatedAt)
        .offset(offset)
        .limit(limit)
        .all(db)
        .await?
        .into_iter()
        .map(activity_response)
        .collect();

    Ok((logs, total))
}

pub async fn get_recent_activity(
    db: &DatabaseConnection,
    user_id: Uuid,
    limit: u64,
) -> Result<Vec<AuthActivityLogResponse>, AuthError> {
    let logs = auth_activity_logs::Entity::find()
        .filter(auth_activity_logs::Column::UserId.eq(user_id))
        .order_by_desc(auth_activity_logs::Column::CreatedAt)
        .limit(limit)
        .all(db)
        .await?
        .into_iter()
        .map(activity_response)
        .collect();

    Ok(logs)
}

pub async fn get_failed_logins(
    db: &DatabaseConnection,
    since_hours: i64,
    limit: u64,
    offset: u64,
) -> Result<(Vec<AuthActivityLogResponse>, i64), AuthError> {
    let since = Utc::now() - Duration::hours(since_hours);
    let query = auth_activity_logs::Entity::find()
        .filter(auth_activity_logs::Column::ActivityType.eq("login_failed"))
        .filter(auth_activity_logs::Column::CreatedAt.gte(since));

    let total = query.clone().count(db).await? as i64;
    let logs = query
        .order_by_desc(auth_activity_logs::Column::CreatedAt)
        .offset(offset)
        .limit(limit)
        .all(db)
        .await?
        .into_iter()
        .map(activity_response)
        .collect();

    Ok((logs, total))
}

// ============================================================================
// GitHub OAuth
// ============================================================================

const GITHUB_AUTHORIZE_URL: &str = "https://github.com/login/oauth/authorize";
const GITHUB_ACCESS_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
const GITHUB_USER_URL: &str = "https://api.github.com/user";
const GITHUB_USER_EMAILS_URL: &str = "https://api.github.com/user/emails";
const OAUTH_EXCHANGE_TTL_MINUTES: i64 = 2;

/// GitHub user profile as returned by `GET https://api.github.com/user`.
/// Mirrors echobackend's `service.GithubUser`.
#[derive(Debug, Deserialize)]
pub struct GithubUser {
    pub login: String,
    pub id: i64,
    pub avatar_url: Option<String>,
    pub email: Option<String>,
    pub name: Option<String>,
    pub html_url: Option<String>,
}

struct OAuthExchangeEntry {
    response: AuthTokenResponse,
    expires_at: chrono::DateTime<Utc>,
}

static OAUTH_EXCHANGE_CODES: OnceLock<Mutex<HashMap<String, OAuthExchangeEntry>>> = OnceLock::new();

fn oauth_exchange_store() -> &'static Mutex<HashMap<String, OAuthExchangeEntry>> {
    OAUTH_EXCHANGE_CODES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn github_http_client() -> Result<reqwest::Client, AuthError> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(AuthError::Request)
}

/// Build the GitHub authorization URL for the OAuth redirect endpoint.
pub fn github_oauth_url(state: &str) -> String {
    let config = GitHubConfig::get();
    let mut url = reqwest::Url::parse(GITHUB_AUTHORIZE_URL).expect("GitHub authorize URL is valid");
    url.query_pairs_mut()
        .append_pair("client_id", &config.client_id)
        .append_pair("redirect_uri", &config.redirect_uri)
        .append_pair("scope", "user:email")
        .append_pair("state", state);
    url.to_string()
}

/// Exchange a GitHub authorization code for a GitHub access token.
pub async fn get_github_token(code: &str) -> Result<String, AuthError> {
    let config = GitHubConfig::get();
    let response = github_http_client()?
        .post(GITHUB_ACCESS_TOKEN_URL)
        .header(reqwest::header::ACCEPT, "application/json")
        .form(&[
            ("client_id", config.client_id.as_str()),
            ("client_secret", config.client_secret.as_str()),
            ("code", code),
            ("redirect_uri", config.redirect_uri.as_str()),
        ])
        .send()
        .await?;

    let body: serde_json::Value = response.json().await?;
    body.get("access_token")
        .and_then(|value| value.as_str())
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .ok_or_else(|| AuthError::OAuth("no access_token in GitHub response".to_string()))
}

/// Fetch the GitHub profile for the given GitHub access token.
pub async fn fetch_github_user(token: &str) -> Result<GithubUser, AuthError> {
    let response = github_http_client()?
        .get(GITHUB_USER_URL)
        .header(reqwest::header::AUTHORIZATION, format!("Bearer {token}"))
        .header(reqwest::header::ACCEPT, "application/vnd.github.v3+json")
        .header(reqwest::header::USER_AGENT, "pilput-axumbackend")
        .send()
        .await?;

    if response.status() != reqwest::StatusCode::OK {
        return Err(AuthError::OAuth(format!(
            "GitHub API returned status {}",
            response.status()
        )));
    }

    Ok(response.json::<GithubUser>().await?)
}

/// Fetch the primary email of the GitHub account, falling back to the first
/// email in the list. Mirrors echobackend's `fetchGithubUserEmail`.
pub async fn fetch_github_user_email(token: &str) -> Result<Option<String>, AuthError> {
    #[derive(Debug, Deserialize)]
    struct GithubEmail {
        email: String,
        primary: bool,
    }

    let response = github_http_client()?
        .get(GITHUB_USER_EMAILS_URL)
        .header(reqwest::header::AUTHORIZATION, format!("Bearer {token}"))
        .header(reqwest::header::ACCEPT, "application/vnd.github.v3+json")
        .header(reqwest::header::USER_AGENT, "pilput-axumbackend")
        .send()
        .await?;

    if response.status() != reqwest::StatusCode::OK {
        return Err(AuthError::OAuth(format!(
            "GitHub API returned status {}",
            response.status()
        )));
    }

    let emails: Vec<GithubEmail> = response.json().await?;
    if let Some(primary) = emails.iter().find(|entry| entry.primary) {
        return Ok(Some(primary.email.clone()));
    }
    Ok(emails.into_iter().next().map(|entry| entry.email))
}

/// Sign in (or register) a user from a GitHub profile and issue tokens.
/// Mirrors echobackend's `authService.SignInWithGithub`.
pub async fn sign_in_with_github(
    db: &DatabaseConnection,
    github_user: &GithubUser,
    user_agent: Option<String>,
) -> Result<AuthTokenResponse, AuthError> {
    let existing = users::Entity::find()
        .filter(users::Column::GithubId.eq(github_user.id))
        .filter(users::Column::DeletedAt.is_null())
        .one(db)
        .await?;

    let user = match existing {
        Some(user) => user,
        None => {
            let email = github_user
                .email
                .clone()
                .filter(|email| !email.is_empty())
                .unwrap_or_else(|| format!("{}@github.placeholder", github_user.id));

            let new_user = users::ActiveModel {
                id: Set(Uuid::new_v4()),
                email: Set(email),
                username: Set(Some(github_user.login.clone())),
                github_id: Set(Some(github_user.id)),
                image: Set(github_user.avatar_url.clone()),
                created_at: Set(Some(Utc::now().into())),
                updated_at: Set(Some(Utc::now().into())),
                ..Default::default()
            };

            match new_user.insert(db).await {
                Ok(user) => user,
                Err(err) => {
                    log_activity(
                        db,
                        None,
                        "oauth_login_failed",
                        "failure",
                        None,
                        user_agent,
                        None,
                        Some(serde_json::json!({
                            "provider": "github",
                            "error": err.to_string(),
                        })),
                    )
                    .await;
                    return Err(AuthError::Db(err));
                }
            }
        }
    };

    let response = match create_token_and_session(db, &user, user_agent.clone()).await {
        Ok(response) => response,
        Err(err) => {
            log_activity(
                db,
                Some(user.id),
                "oauth_login_failed",
                "failure",
                None,
                user_agent,
                None,
                Some(serde_json::json!({ "provider": "github" })),
            )
            .await;
            return Err(err);
        }
    };

    log_activity(
        db,
        Some(user.id),
        "oauth_login",
        "success",
        None,
        user_agent,
        None,
        Some(serde_json::json!({ "provider": "github" })),
    )
    .await;

    let mut active_user: users::ActiveModel = user.clone().into();
    active_user.last_logged_at = Set(Some(Utc::now().into()));
    let _ = active_user.update(db).await;

    Ok(response)
}

/// Create a one-time OAuth exchange code (2-minute TTL) holding the issued
/// tokens. Mirrors echobackend's in-memory fallback of `CreateOAuthExchangeCode`.
pub fn create_oauth_exchange_code(response: AuthTokenResponse) -> String {
    let code = generate_prefixed_token("oc");
    let now = Utc::now();
    let mut store = oauth_exchange_store()
        .lock()
        .expect("oauth exchange store lock poisoned");
    store.retain(|_, entry| entry.expires_at > now);
    store.insert(
        code.clone(),
        OAuthExchangeEntry {
            response,
            expires_at: now + Duration::minutes(OAUTH_EXCHANGE_TTL_MINUTES),
        },
    );
    code
}

/// Atomically redeem and delete a one-time OAuth exchange code.
/// Mirrors echobackend's `ExchangeOAuthCode`.
pub fn exchange_oauth_code(code: &str) -> Result<AuthTokenResponse, AuthError> {
    let now = Utc::now();
    let mut store = oauth_exchange_store()
        .lock()
        .expect("oauth exchange store lock poisoned");
    store.retain(|_, entry| entry.expires_at > now);
    store
        .remove(code)
        .map(|entry| entry.response)
        .ok_or(AuthError::InvalidToken)
}
