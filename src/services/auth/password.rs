use super::*;

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

pub(super) fn has_current_argon2_params(hash: &PasswordHash) -> bool {
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
pub(super) async fn hash_password_async(password: &str) -> Result<String, AuthError> {
    let password = password.to_owned();
    tokio::task::spawn_blocking(move || hash_password(&password))
        .await
        .map_err(|e| AuthError::Hash(e.to_string()))?
}

/// Runs [`verify_password`] on the blocking pool (see [`hash_password_async`]).
pub(super) async fn verify_password_async(hashed: &str, password: &str) -> (bool, bool) {
    let (hashed, password) = (hashed.to_owned(), password.to_owned());
    tokio::task::spawn_blocking(move || verify_password(&hashed, &password))
        .await
        .unwrap_or((false, false))
}

pub async fn forgot_password(
    db: &DatabaseConnection,
    mailer: &dyn Mailer,
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

    if mailer.is_configured() {
        if let Err(err) = mailer.send_password_reset_email(email, &reset_link).await {
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

pub(super) fn build_password_reset_link(token: &str) -> String {
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
