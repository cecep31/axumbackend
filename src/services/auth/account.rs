use super::*;

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
        first_name: Set(Some(first_name)),
        last_name: Set(Some(last_name)),
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
        first_name: user.first_name,
        last_name: user.last_name,
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
