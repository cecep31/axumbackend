use super::*;

pub(super) fn generate_refresh_token() -> String {
    let random = Alphanumeric.sample_string(&mut rand::rng(), 86);
    format!("pl_{}", random)
}

pub(super) fn generate_prefixed_token(prefix: &str) -> String {
    let random = Alphanumeric.sample_string(&mut rand::rng(), 64);
    format!("{}_{}", prefix, random)
}

/// SHA-256 hex digest of a refresh token. Only the digest is stored, matching
/// echobackend's `tokenHash`, so a leaked sessions table yields no usable tokens.
pub(super) fn token_hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

/// Payload of a minted access token (echobackend's `jwt.MapClaims{user_id,
/// iat, exp}`). Decoding stays tolerant via [`crate::http::auth::Claims`].
#[derive(Debug, Serialize, Deserialize)]
pub(super) struct AccessTokenClaims {
    pub user_id: String,
    pub iat: usize,
    pub exp: usize,
}

pub(super) fn create_access_token(user: &users::Model) -> Result<String, AuthError> {
    let now = Utc::now();
    let jwt = JwtConfig::get();
    let expiry_duration = Duration::from_std(jwt.expiry).unwrap_or_else(|_| Duration::minutes(15));
    let exp = now + expiry_duration;
    // echobackend `createAccessToken`: HS256 with exactly user_id, iat, exp.
    // Identity details and the admin flag are read from the database, never
    // from the token.
    let claims = AccessTokenClaims {
        user_id: user.id.to_string(),
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
pub(super) fn build_session(
    user_id: Uuid,
    family_id: Option<Uuid>,
    absolute_expires_at: DateTime<Utc>,
    client: ClientInfo,
) -> (String, sessions::Model) {
    let refresh_token = generate_refresh_token();
    let now = Utc::now();
    // The sliding window never outlives the absolute cap, so refreshing right
    // before the deadline does not gain the chain extra time.
    let expires_at = std::cmp::min(
        now + Duration::from_std(JwtConfig::get().refresh_token_expiry)
            .unwrap_or_else(|_| Duration::hours(72)),
        absolute_expires_at,
    );
    let id = Uuid::now_v7();

    let session = sessions::Model {
        id,
        family_id: family_id.unwrap_or(id),
        refresh_token: token_hash(&refresh_token),
        user_id,
        user_agent: client.user_agent,
        // echobackend stores the client IP only when it is known.
        ip_address: client.ip_address.filter(|ip| !ip.is_empty()),
        created_at: now.into(),
        expires_at: expires_at.into(),
        absolute_expires_at: absolute_expires_at.into(),
        rotated_at: None,
        replaced_by: None,
    };
    (refresh_token, session)
}

pub(super) fn new_session(session: sessions::Model) -> sessions::ActiveModel {
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
pub(super) async fn create_token_and_session(
    db: &DatabaseConnection,
    user: &users::Model,
    client: ClientInfo,
) -> Result<AuthTokenResponse, AuthError> {
    let access_token = create_access_token(user)?;

    let absolute_expiry = Duration::from_std(JwtConfig::get().refresh_token_absolute_expiry)
        .unwrap_or_else(|_| Duration::days(30));
    let (refresh_token, session) =
        build_session(user.id, None, Utc::now() + absolute_expiry, client);
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
pub(super) async fn prune_expired_sessions(db: &DatabaseConnection, user_id: Uuid) {
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

pub(super) async fn delete_session_family(
    db: &DatabaseConnection,
    family_id: Uuid,
) -> Result<(), DbErr> {
    sessions::Entity::delete_many()
        .filter(sessions::Column::FamilyId.eq(family_id))
        .exec(db)
        .await?;
    Ok(())
}

pub(super) fn refresh_token_grace() -> Duration {
    Duration::from_std(JwtConfig::get().refresh_token_grace_period)
        .unwrap_or_else(|_| Duration::zero())
}

#[derive(Debug)]
pub enum RevokeError {
    Db(DbErr),
    UserNotFound,
}

impl From<DbErr> for RevokeError {
    fn from(err: DbErr) -> Self {
        Self::Db(err)
    }
}

/// Terminates every session of one user (admin only, mirrors echobackend
/// `RevokeUserSessions`). Returns the number of sessions killed.
pub async fn revoke_user_sessions(
    db: &DatabaseConnection,
    target_user_id: Uuid,
    actor_id: Uuid,
    client: ClientInfo,
) -> Result<u64, RevokeError> {
    // echobackend `userRepo.GetByID(ctx, id, false)`: soft-deleted users are
    // not found, so nothing is revoked and no activity is logged for them.
    users::Entity::find_by_id(target_user_id)
        .filter(users::Column::DeletedAt.is_null())
        .one(db)
        .await?
        .ok_or(RevokeError::UserNotFound)?;
    let result = sessions::Entity::delete_many()
        .filter(sessions::Column::UserId.eq(target_user_id))
        .exec(db)
        .await?;
    log_activity(
        db,
        Some(target_user_id),
        "session_revoked",
        "success",
        client.ip_address.clone(),
        client.user_agent.clone(),
        None,
        Some(serde_json::json!({"revoked_by": actor_id, "revoked_sessions": result.rows_affected})),
    )
    .await;
    Ok(result.rows_affected)
}

/// Terminates every session of every user, including the acting admin
/// (mirrors echobackend `RevokeAllSessions`).
pub async fn revoke_all_sessions(
    db: &DatabaseConnection,
    actor_id: Uuid,
    client: ClientInfo,
) -> Result<u64, RevokeError> {
    let result = sessions::Entity::delete_many().exec(db).await?;
    log_activity(
        db,
        None,
        "session_revoked",
        "success",
        client.ip_address.clone(),
        client.user_agent.clone(),
        None,
        Some(
            serde_json::json!({"revoked_by": actor_id, "revoked_sessions": result.rows_affected, "scope": "all_users"}),
        ),
    )
    .await;
    Ok(result.rows_affected)
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
    client: ClientInfo,
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
            return issue_rotated_token(db, &session, false, client).await;
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
            client.ip_address.clone(),
            client.user_agent.clone(),
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

    issue_rotated_token(db, &session, true, client).await
}

/// Mints the successor of `current` along with a fresh access token.
///
/// With `rotate` the successor replaces `current` atomically; without it (the
/// grace path) the successor is inserted as a sibling, since `current` was
/// already rotated by the request that won the race.
pub(super) async fn issue_rotated_token(
    db: &DatabaseConnection,
    current: &sessions::Model,
    rotate: bool,
    client: ClientInfo,
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
        client.clone(),
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
        client.ip_address.clone(),
        client.user_agent.clone(),
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
    client: ClientInfo,
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
        client.ip_address.clone(),
        client.user_agent.clone(),
        None,
        None,
    )
    .await;
    Ok(())
}
