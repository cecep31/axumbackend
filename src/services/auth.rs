use crate::auth::Claims;
use crate::config::{GitHubConfig, JwtConfig};
use crate::email;
use crate::entities::{auth_activity_logs, password_reset_tokens, sessions, users};
use argon2::{
    Argon2, Params,
    password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash},
};
use chrono::{DateTime, Duration, Utc};
use jsonwebtoken::{EncodingKey, Header, encode};
use rand::distr::{Alphanumeric, SampleString};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, DatabaseConnection, DbErr, EntityTrait, ExprTrait,
    PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, Set, TransactionTrait,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
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
    Hash(String),
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
        Self::Hash(err.to_string())
    }
}

fn generate_refresh_token() -> String {
    let random = Alphanumeric.sample_string(&mut rand::rng(), 86);
    format!("pl_{}", random)
}

fn generate_prefixed_token(prefix: &str) -> String {
    let random = Alphanumeric.sample_string(&mut rand::rng(), 64);
    format!("{}_{}", prefix, random)
}

// Argon2id parameters aligned with OWASP, matching echobackend's
// `pkg/password` defaults.
const ARGON2_MEMORY_KIB: u32 = 64 * 1024;
const ARGON2_TIME: u32 = 1;
const ARGON2_THREADS: u32 = 4;
const ARGON2_KEY_LEN: usize = 32;

/// Hashes a password using Argon2id with OWASP-aligned parameters
/// (memory: 64MB, time: 1 iteration, threads: 4, key_len: 32 bytes),
/// matching echobackend's `pkg/password.Hash`.
pub fn hash_password(password: &str) -> Result<String, AuthError> {
    let params = Params::new(
        ARGON2_MEMORY_KIB,
        ARGON2_TIME,
        ARGON2_THREADS,
        Some(ARGON2_KEY_LEN),
    )
    .map_err(|e| AuthError::Hash(e.to_string()))?;
    let argon2 = Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    let hash = argon2
        .hash_password(password.as_bytes())
        .map_err(|e| AuthError::Hash(e.to_string()))?;
    Ok(hash.to_string())
}

/// Verifies whether `password` matches `hashed`.
/// Supports both Argon2id ($argon2id$) and legacy bcrypt ($2a$, $2b$, $2y$).
/// Returns `(is_valid, needs_rehash)`; `needs_rehash` is set for bcrypt and for
/// Argon2id hashes whose parameters differ from the current defaults, like
/// echobackend's `NeedsRehash`.
pub fn verify_password(hashed: &str, password: &str) -> (bool, bool) {
    if hashed.starts_with("$argon2id$") {
        if let Ok(parsed_hash) = PasswordHash::new(hashed) {
            let argon2 = Argon2::default();
            let valid = argon2
                .verify_password(password.as_bytes(), &parsed_hash)
                .is_ok();
            return (valid, valid && !has_current_argon2_params(&parsed_hash));
        }
        return (false, false);
    }

    if (hashed.starts_with("$2a$") || hashed.starts_with("$2b$") || hashed.starts_with("$2y$"))
        && let Ok(valid) = bcrypt::verify(password, hashed)
    {
        return (valid, valid);
    }

    (false, false)
}

fn has_current_argon2_params(hash: &PasswordHash) -> bool {
    let Ok(params) = Params::try_from(hash) else {
        return false;
    };
    params.m_cost() == ARGON2_MEMORY_KIB
        && params.t_cost() == ARGON2_TIME
        && params.p_cost() == ARGON2_THREADS
        && hash.hash.as_ref().map(|h| h.len()) == Some(ARGON2_KEY_LEN)
}

/// Runs [`hash_password`] on the blocking pool: Argon2id with 64MB memory is
/// CPU/memory heavy and would otherwise stall a tokio worker thread.
async fn hash_password_async(password: &str) -> Result<String, AuthError> {
    let password = password.to_owned();
    tokio::task::spawn_blocking(move || hash_password(&password))
        .await
        .map_err(|e| AuthError::Hash(e.to_string()))?
}

/// Runs [`verify_password`] on the blocking pool (see [`hash_password_async`]).
async fn verify_password_async(hashed: &str, password: &str) -> (bool, bool) {
    let (hashed, password) = (hashed.to_owned(), password.to_owned());
    tokio::task::spawn_blocking(move || verify_password(&hashed, &password))
        .await
        .unwrap_or((false, false))
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
        id: Set(Uuid::now_v7()),
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

/// SHA-256 hex digest of a refresh token. Only the digest is stored, matching
/// echobackend's `tokenHash`, so a leaked sessions table yields no usable tokens.
fn token_hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

fn create_access_token(user: &users::Model) -> Result<String, AuthError> {
    let now = Utc::now();
    let jwt = JwtConfig::get();
    let expiry_duration = Duration::from_std(jwt.expiry).unwrap_or_else(|_| Duration::minutes(15));
    let exp = now + expiry_duration;
    let claims = Claims {
        user_id: user.id,
        username: user.username.clone(),
        email: user.email.clone(),
        is_super_admin: user.is_super_admin,
        iat: now.timestamp() as usize,
        exp: exp.timestamp() as usize,
    };

    Ok(encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(jwt.secret.as_bytes()),
    )?)
}

/// Mints a refresh token and the row that will hold its hash, returning the
/// raw token (the only time it exists in plaintext) and the unsaved session.
///
/// `None` for `family_id` starts a new family headed by the row's own id.
fn build_session(
    user_id: Uuid,
    family_id: Option<Uuid>,
    absolute_expires_at: DateTime<Utc>,
    user_agent: Option<String>,
) -> (String, sessions::Model) {
    let refresh_token = generate_refresh_token();
    let now = Utc::now();
    // The sliding window never outlives the absolute cap, so refreshing right
    // before the deadline does not gain the chain extra time.
    let expires_at = std::cmp::min(
        now + Duration::days(JwtConfig::get().refresh_token_expiry_days),
        absolute_expires_at,
    );
    let id = Uuid::now_v7();

    let session = sessions::Model {
        id,
        family_id: family_id.unwrap_or(id),
        refresh_token: token_hash(&refresh_token),
        user_id,
        user_agent,
        ip_address: None,
        created_at: now.into(),
        expires_at: expires_at.into(),
        absolute_expires_at: absolute_expires_at.into(),
        rotated_at: None,
        replaced_by: None,
    };
    (refresh_token, session)
}

fn new_session(session: sessions::Model) -> sessions::ActiveModel {
    sessions::ActiveModel {
        id: Set(session.id),
        family_id: Set(session.family_id),
        refresh_token: Set(session.refresh_token),
        user_id: Set(session.user_id),
        user_agent: Set(session.user_agent),
        ip_address: Set(session.ip_address),
        created_at: Set(session.created_at),
        expires_at: Set(session.expires_at),
        absolute_expires_at: Set(session.absolute_expires_at),
        rotated_at: Set(session.rotated_at),
        replaced_by: Set(session.replaced_by),
    }
}

/// Starts a new rotation chain: the session it creates is the root of its own
/// family and sets the absolute deadline every later rotation inherits.
async fn create_token_and_session(
    db: &DatabaseConnection,
    user: &users::Model,
    user_agent: Option<String>,
) -> Result<AuthTokenResponse, AuthError> {
    let access_token = create_access_token(user)?;

    let absolute_expiry = Duration::from_std(JwtConfig::get().refresh_token_absolute_expiry)
        .unwrap_or_else(|_| Duration::days(30));
    let (refresh_token, session) =
        build_session(user.id, None, Utc::now() + absolute_expiry, user_agent);
    new_session(session).insert(db).await?;

    prune_expired_sessions(db, user.id).await;

    Ok(AuthTokenResponse {
        access_token,
        refresh_token,
        user: make_user_brief(user),
    })
}

/// Drops the user's dead rows. Rotation appends a row per refresh, so without
/// this the table would grow with every access-token renewal. Best-effort: a
/// failure here must not fail the caller.
async fn prune_expired_sessions(db: &DatabaseConnection, user_id: Uuid) {
    let now = Utc::now();
    // Rotated rows are covered by expires_at too: past it the token could no
    // longer have been redeemed, so keeping it for replay detection buys nothing.
    let result = sessions::Entity::delete_many()
        .filter(sessions::Column::UserId.eq(user_id))
        .filter(
            Condition::any()
                .add(sessions::Column::ExpiresAt.lt(now))
                .add(sessions::Column::AbsoluteExpiresAt.lt(now)),
        )
        .exec(db)
        .await;
    if let Err(err) = result {
        tracing::warn!(?err, %user_id, "failed to prune expired sessions");
    }
}

async fn delete_session_family(db: &DatabaseConnection, family_id: Uuid) -> Result<(), DbErr> {
    sessions::Entity::delete_many()
        .filter(sessions::Column::FamilyId.eq(family_id))
        .exec(db)
        .await?;
    Ok(())
}

fn refresh_token_grace() -> Duration {
    Duration::from_std(JwtConfig::get().refresh_token_grace_period)
        .unwrap_or_else(|_| Duration::zero())
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

    let hashed = hash_password_async(&password).await?;
    let user = users::ActiveModel {
        id: Set(Uuid::now_v7()),
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

    let (is_valid, needs_rehash) = verify_password_async(hashed_password, password).await;
    if !is_valid {
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
    if needs_rehash && let Ok(new_hash) = hash_password_async(password).await {
        active.password = Set(Some(new_hash));
    }
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

pub async fn check_username_exists(
    db: &DatabaseConnection,
    username: &str,
) -> Result<bool, AuthError> {
    let exists = users::Entity::find()
        .filter(users::Column::Username.eq(username))
        .filter(users::Column::DeletedAt.is_null())
        .one(db)
        .await?
        .is_some();

    Ok(exists)
}

pub async fn check_username_availability(
    db: &DatabaseConnection,
    username: &str,
) -> Result<bool, AuthError> {
    let exists = check_username_exists(db, username).await?;
    Ok(!exists)
}

pub async fn update_profile(
    db: &DatabaseConnection,
    user_id: Uuid,
    username: String,
    first_name: Option<String>,
    last_name: Option<String>,
) -> Result<ProfileResponse, AuthError> {
    let user = users::Entity::find_by_id(user_id)
        .filter(users::Column::DeletedAt.is_null())
        .one(db)
        .await?
        .ok_or(AuthError::InvalidToken)?;

    // Check username uniqueness if changed
    if user.username.as_deref() != Some(&username) {
        let existing = users::Entity::find()
            .filter(users::Column::Username.eq(&username))
            .filter(users::Column::Id.ne(user_id))
            .filter(users::Column::DeletedAt.is_null())
            .one(db)
            .await?;
        if existing.is_some() {
            return Err(AuthError::UserExists);
        }
    }

    let mut active: users::ActiveModel = user.clone().into();
    active.username = Set(Some(username.clone()));
    active.first_name = Set(first_name.clone());
    active.last_name = Set(last_name.clone());
    active.updated_at = Set(Some(Utc::now().into()));
    let updated = active.update(db).await?;

    Ok(ProfileResponse {
        id: updated.id,
        email: updated.email,
        username: updated.username,
        first_name: updated.first_name,
        last_name: updated.last_name,
        image: updated.image,
        is_super_admin: updated.is_super_admin,
        followers_count: updated.followers_count.unwrap_or_default(),
        following_count: updated.following_count.unwrap_or_default(),
    })
}

pub async fn delete_account(db: &DatabaseConnection, user_id: Uuid) -> Result<(), AuthError> {
    let user = users::Entity::find_by_id(user_id)
        .filter(users::Column::DeletedAt.is_null())
        .one(db)
        .await?
        .ok_or(AuthError::InvalidToken)?;

    let mut active: users::ActiveModel = user.into();
    active.deleted_at = Set(Some(Utc::now().into()));
    active.updated_at = Set(Some(Utc::now().into()));
    active.update(db).await?;

    let _ = sessions::Entity::delete_many()
        .filter(sessions::Column::UserId.eq(user_id))
        .exec(db)
        .await;

    Ok(())
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

/// Exchanges a refresh token for a fresh access token and a fresh refresh
/// token, rotating the chain forward (RFC 9700 §4.14), same as echobackend.
///
/// The returned refresh token always replaces the one that was sent. A token
/// presented after it was already exchanged is treated as a replay and takes
/// its entire family down with it.
pub async fn refresh_token(
    db: &DatabaseConnection,
    refresh_token: &str,
    user_agent: Option<String>,
) -> Result<AuthTokenResponse, AuthError> {
    let session = sessions::Entity::find()
        .filter(sessions::Column::RefreshToken.eq(token_hash(refresh_token)))
        .one(db)
        .await?
        .ok_or(AuthError::InvalidToken)?;

    let now = Utc::now();

    // Checked first: once a family is past its maximum lifetime nothing in it
    // can be revived, not even through the grace window below.
    if now > session.absolute_expires_at.with_timezone(&Utc) {
        if let Err(err) = delete_session_family(db, session.family_id).await {
            tracing::warn!(?err, user_id = %session.user_id, "failed to delete session family past absolute expiry");
        }
        return Err(AuthError::TokenExpired);
    }

    if let Some(rotated_at) = session.rotated_at {
        // Inside the grace window this is almost certainly the client's own
        // concurrent refresh, so it is served rather than punished. Grace is
        // anchored to the first rotation, so replays cannot push it forward.
        let grace = refresh_token_grace();
        if grace > Duration::zero() && now <= rotated_at.with_timezone(&Utc) + grace {
            return issue_rotated_token(db, &session, false, user_agent).await;
        }

        // Past the window, assume the token leaked: whoever holds the successor
        // may be the attacker, so the whole chain goes.
        if let Err(err) = delete_session_family(db, session.family_id).await {
            tracing::error!(?err, user_id = %session.user_id, family_id = %session.family_id, "failed to revoke session family after refresh token reuse");
        }
        log_activity(
            db,
            Some(session.user_id),
            "token_reuse_detected",
            "failure",
            None,
            user_agent,
            None,
            Some(serde_json::json!({ "family_id": session.family_id })),
        )
        .await;
        tracing::warn!(user_id = %session.user_id, family_id = %session.family_id, "refresh token reuse detected, session family revoked");
        return Err(AuthError::InvalidToken);
    }

    if now > session.expires_at.with_timezone(&Utc) {
        // Only this leaf is dead. Siblings minted during a grace window carry
        // their own deadlines, so the family is left alone.
        let _ = sessions::Entity::delete_by_id(session.id).exec(db).await;
        return Err(AuthError::TokenExpired);
    }

    issue_rotated_token(db, &session, true, user_agent).await
}

/// Mints the successor of `current` along with a fresh access token.
///
/// With `rotate` the successor replaces `current` atomically; without it (the
/// grace path) the successor is inserted as a sibling, since `current` was
/// already rotated by the request that won the race.
async fn issue_rotated_token(
    db: &DatabaseConnection,
    current: &sessions::Model,
    rotate: bool,
    user_agent: Option<String>,
) -> Result<AuthTokenResponse, AuthError> {
    // The account may have been deleted after the session was issued.
    let user = users::Entity::find_by_id(current.user_id)
        .filter(users::Column::DeletedAt.is_null())
        .one(db)
        .await?
        .ok_or(AuthError::InvalidToken)?;

    let access_token = create_access_token(&user)?;
    let (refresh_token, next) = build_session(
        user.id,
        Some(current.family_id),
        current.absolute_expires_at.with_timezone(&Utc),
        user_agent.clone(),
    );

    if rotate {
        let txn = db.begin().await?;
        // The rotated_at IS NULL predicate is the concurrency control: two
        // parallel refreshes read the same row, but only one UPDATE matches.
        let result = sessions::Entity::update_many()
            .set(sessions::ActiveModel {
                rotated_at: Set(Some(next.created_at)),
                replaced_by: Set(Some(next.refresh_token.clone())),
                ..Default::default()
            })
            .filter(sessions::Column::Id.eq(current.id))
            .filter(sessions::Column::RotatedAt.is_null())
            .exec(&txn)
            .await?;

        if result.rows_affected == 0 {
            txn.rollback().await?;
            // A concurrent refresh rotated this row between our read and our
            // write. With a grace window that is the same benign race handled
            // in refresh_token, so mint a sibling instead of failing.
            if refresh_token_grace() <= Duration::zero() {
                return Err(AuthError::InvalidToken);
            }
            new_session(next).insert(db).await?;
        } else {
            new_session(next).insert(&txn).await?;
            txn.commit().await?;
        }
    } else {
        new_session(next).insert(db).await?;
    }

    prune_expired_sessions(db, user.id).await;
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

    Ok(AuthTokenResponse {
        access_token,
        refresh_token,
        user: make_user_brief(&user),
    })
}

/// Ends the whole rotation chain the token belongs to, not just the token
/// itself, so any sibling minted during a grace window dies with it
/// (ASVS 7.4.1). Unknown tokens are a no-op: logging out is idempotent.
pub async fn logout(
    db: &DatabaseConnection,
    user_id: Uuid,
    refresh_token: &str,
    user_agent: Option<String>,
) -> Result<(), AuthError> {
    let session = sessions::Entity::find()
        .filter(sessions::Column::RefreshToken.eq(token_hash(refresh_token)))
        .filter(sessions::Column::UserId.eq(user_id))
        .one(db)
        .await?;
    if let Some(session) = session {
        delete_session_family(db, session.family_id).await?;
    }
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
        id: Set(Uuid::now_v7()),
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

    let hashed = hash_password_async(password).await?;
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

    let (is_valid, _) = verify_password_async(hashed_password, current_password).await;
    if !is_valid {
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

    let hashed = hash_password_async(new_password).await?;
    let mut active_user: users::ActiveModel = user.clone().into();
    active_user.password = Set(Some(hashed));
    active_user.updated_at = Set(Some(Utc::now().into()));
    active_user.update(db).await?;

    // Revoke every refresh token so sessions opened with the old password
    // (possibly by someone else) cannot be extended.
    if let Err(err) = sessions::Entity::delete_many()
        .filter(sessions::Column::UserId.eq(user.id))
        .exec(db)
        .await
    {
        tracing::warn!(?err, user_id = %user.id, "failed to invalidate sessions after password change");
    }

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
                id: Set(Uuid::now_v7()),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_argon2id_hash_and_verify() {
        let password = "SecretPassword123!";
        let hashed = hash_password(password).expect("hashing should succeed");
        assert!(hashed.starts_with("$argon2id$"));

        let (valid, needs_rehash) = verify_password(&hashed, password);
        assert!(valid);
        assert!(!needs_rehash);

        let (invalid, _) = verify_password(&hashed, "WrongPassword");
        assert!(!invalid);
    }

    #[test]
    fn test_argon2id_outdated_params_need_rehash() {
        let password = "SecurePassword123!";
        let params = Params::new(19 * 1024, 2, 1, Some(32)).unwrap();
        let old_hash = Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params)
            .hash_password(password.as_bytes())
            .unwrap()
            .to_string();

        let (valid, needs_rehash) = verify_password(&old_hash, password);
        assert!(valid);
        assert!(needs_rehash);

        let (invalid, needs_rehash) = verify_password(&old_hash, "WrongPassword");
        assert!(!invalid);
        assert!(!needs_rehash);
    }

    #[test]
    fn test_token_hash_matches_echobackend() {
        // echobackend: hex(sha256(token))
        assert_eq!(
            token_hash("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn test_legacy_bcrypt_verify_and_rehash_flag() {
        let password = "LegacyBcryptPassword123!";
        let bcrypt_hash = bcrypt::hash(password, 4).expect("bcrypt hash should succeed");
        assert!(bcrypt_hash.starts_with("$2"));

        let (valid, needs_rehash) = verify_password(&bcrypt_hash, password);
        assert!(valid);
        assert!(needs_rehash);

        let (invalid, _) = verify_password(&bcrypt_hash, "WrongPassword");
        assert!(!invalid);
    }

    #[test]
    fn test_oauth_exchange_roundtrip() {
        let token_resp = AuthTokenResponse {
            access_token: "access-123".into(),
            refresh_token: "pl_test".into(),
            user: UserBrief {
                id: Uuid::now_v7(),
                email: "test@example.com".into(),
                username: Some("testuser".into()),
            },
        };
        let code = create_oauth_exchange_code(token_resp);
        assert!(code.starts_with("oc_"));

        let redeemed = exchange_oauth_code(&code).expect("should exchange successfully");
        assert_eq!(redeemed.access_token, "access-123");

        // Second exchange should fail (single-use)
        let second = exchange_oauth_code(&code);
        assert!(matches!(second, Err(AuthError::InvalidToken)));
    }
}
