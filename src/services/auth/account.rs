use super::*;

/// echobackend `normalizeEmail`: emails are stored trimmed and lowercased so
/// addresses differing only in case or surrounding spaces map to one account.
pub fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

/// `LOWER(email) = LOWER($1)` (echobackend `FindUserByEmail`), served by the
/// `idx_users_email_lower` index so legacy mixed-case rows still match.
pub fn email_matches(email: &str) -> sea_orm::sea_query::SimpleExpr {
    sea_orm::sea_query::Expr::cust_with_values(
        r#"LOWER("users"."email") = LOWER($1)"#,
        [email.to_owned()],
    )
}

/// Live (not soft-deleted) user whose email matches case-insensitively.
pub(super) async fn find_user_by_email(
    db: &DatabaseConnection,
    email: &str,
) -> Result<Option<users::Model>, DbErr> {
    users::Entity::find()
        .filter(email_matches(email))
        .filter(users::Column::DeletedAt.is_null())
        .one(db)
        .await
}

/// SQLSTATE 23505 on insert (echobackend maps `gorm.ErrDuplicatedKey` to
/// `ErrUserExists`).
pub fn is_unique_violation(err: &DbErr) -> bool {
    matches!(
        err.sql_err(),
        Some(sea_orm::SqlErr::UniqueConstraintViolation(_))
    )
}

pub(super) fn make_user_brief(user: &users::Model) -> UserBrief {
    UserBrief {
        id: user.id,
        email: user.email.clone(),
        username: user.username.clone(),
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

pub async fn register(
    db: &DatabaseConnection,
    email: String,
    username: String,
    password: String,
    first_name: String,
    last_name: String,
    client: ClientInfo,
) -> Result<RegisterResponse, AuthError> {
    // echobackend `Register`: normalized email, then two checks over live
    // users (email case-insensitive, username exact).
    let email = normalize_email(&email);
    if find_user_by_email(db, &email).await?.is_some() {
        return Err(AuthError::UserExists);
    }
    if check_username_exists(db, &username).await? {
        return Err(AuthError::UserExists);
    }

    let hashed = hash_password_async(&password).await?;
    let inserted = users::ActiveModel {
        id: Set(Uuid::now_v7()),
        email: Set(email),
        username: Set(Some(username)),
        password: Set(Some(hashed)),
        first_name: Set(Some(first_name)),
        last_name: Set(Some(last_name)),
        created_at: Set(Some(Utc::now().into())),
        updated_at: Set(Some(Utc::now().into())),
        ..Default::default()
    }
    .insert(db)
    .await;
    let user = match inserted {
        Ok(user) => user,
        Err(err) if is_unique_violation(&err) => return Err(AuthError::UserExists),
        Err(err) => return Err(err.into()),
    };

    log_activity(
        db,
        Some(user.id),
        "register",
        "success",
        client.ip_address.clone(),
        client.user_agent.clone(),
        None,
        None,
    )
    .await;

    Ok(RegisterResponse {
        id: user.id,
        email: user.email,
        username: user.username,
        first_name: user.first_name,
        last_name: user.last_name,
    })
}

pub async fn login(
    db: &DatabaseConnection,
    identifier: &str,
    password: &str,
    client: ClientInfo,
) -> Result<AuthTokenResponse, AuthError> {
    // echobackend `FindUserByIdentifier`: email case-insensitive, username
    // exact (and untrimmed).
    let user = users::Entity::find()
        .filter(
            Condition::any()
                .add(email_matches(identifier))
                .add(users::Column::Username.eq(identifier)),
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
            client.ip_address.clone(),
            client.user_agent.clone(),
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
            client.ip_address.clone(),
            client.user_agent.clone(),
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
            client.ip_address.clone(),
            client.user_agent.clone(),
            None,
            None,
        )
        .await;
        return Err(AuthError::InvalidCredentials);
    }

    let response = create_token_and_session(db, &user, client.clone()).await?;
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
        client.ip_address.clone(),
        client.user_agent.clone(),
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
        .ok_or(AuthError::UserNotFound)?;

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

    // echobackend binds the names as plain strings: an omitted name is
    // stored and returned as "" rather than NULL.
    let mut active: users::ActiveModel = user.clone().into();
    active.username = Set(Some(username.clone()));
    active.first_name = Set(Some(first_name.unwrap_or_default()));
    active.last_name = Set(Some(last_name.unwrap_or_default()));
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
        .ok_or(AuthError::UserNotFound)?;

    let mut active: users::ActiveModel = user.into();
    active.deleted_at = Set(Some(Utc::now().into()));
    active.updated_at = Set(Some(Utc::now().into()));
    active.update(db).await?;

    if let Err(err) = sessions::Entity::delete_many()
        .filter(sessions::Column::UserId.eq(user_id))
        .exec(db)
        .await
    {
        tracing::warn!(?err, %user_id, "failed to invalidate sessions after account deletion");
    }

    Ok(())
}

pub async fn check_email_availability(
    db: &DatabaseConnection,
    email: &str,
) -> Result<bool, AuthError> {
    let exists = find_user_by_email(db, email).await?.is_some();
    Ok(!exists)
}
