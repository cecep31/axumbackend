use crate::dto::user::UserDeletedFilter;
use crate::entities::users;
use crate::models::user::{UserResponse, UserView};
use crate::services::user_hydration;
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, DatabaseConnection, DbErr, EntityTrait,
    IntoActiveModel, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, Set,
};
use uuid::Uuid;

async fn hydrate_user(
    db: &DatabaseConnection,
    user: users::Model,
    view: UserView,
) -> Result<UserResponse, DbErr> {
    let users_by_id = user_hydration::load_user_response_map(db, [user.id], view).await?;
    Ok(users_by_id
        .get(&user.id)
        .cloned()
        .unwrap_or_else(|| UserResponse::from_entity_with_view(user, None, None, view)))
}

pub async fn get_by_id(db: &DatabaseConnection, id: Uuid) -> Result<Option<UserResponse>, DbErr> {
    let user = users::Entity::find_by_id(id)
        .filter(users::Column::DeletedAt.is_null())
        .one(db)
        .await?;

    match user {
        Some(user) => Ok(Some(hydrate_user(db, user, UserView::General).await?)),
        None => Ok(None),
    }
}

/// Current user viewing themselves — exposes email + super-admin
/// (echobackend `CurrentUserResponse`).
pub async fn get_current_user(
    db: &DatabaseConnection,
    id: Uuid,
) -> Result<Option<UserResponse>, DbErr> {
    let user = users::Entity::find_by_id(id)
        .filter(users::Column::DeletedAt.is_null())
        .one(db)
        .await?;

    match user {
        Some(user) => Ok(Some(hydrate_user(db, user, UserView::Current).await?)),
        None => Ok(None),
    }
}

/// Admin view of a user — exposes email + super-admin + last-login + deletion
/// info (echobackend `UserToAdminResponse`), with `is_following` populated
/// relative to `current_user_id` (echobackend `GetUserWithFollowStatus`).
pub async fn get_admin_by_id(
    db: &DatabaseConnection,
    id: Uuid,
    current_user_id: Option<Uuid>,
) -> Result<Option<UserResponse>, DbErr> {
    let user = users::Entity::find_by_id(id)
        .filter(users::Column::DeletedAt.is_null())
        .one(db)
        .await?;

    match user {
        Some(user) => {
            let mut response = hydrate_user(db, user, UserView::Admin).await?;
            if let Some(current_user_id) = current_user_id
                && current_user_id != id
            {
                response.is_following = Some(
                    crate::services::user_follow::is_following(db, current_user_id, id).await?,
                );
            }
            Ok(Some(response))
        }
        None => Ok(None),
    }
}

/// Admin view of a *soft-deleted* user only (echobackend `GetAdminByID(id, deletedOnly=true)`).
pub async fn get_admin_by_id_deleted_only(
    db: &DatabaseConnection,
    id: Uuid,
) -> Result<Option<UserResponse>, DbErr> {
    let user = users::Entity::find_by_id(id)
        .filter(users::Column::DeletedAt.is_not_null())
        .one(db)
        .await?;

    match user {
        Some(user) => Ok(Some(hydrate_user(db, user, UserView::Admin).await?)),
        None => Ok(None),
    }
}

pub async fn get_by_username(
    db: &DatabaseConnection,
    username: &str,
) -> Result<Option<UserResponse>, DbErr> {
    let user = users::Entity::find()
        .filter(users::Column::Username.eq(username))
        .filter(users::Column::DeletedAt.is_null())
        .one(db)
        .await?;

    match user {
        Some(user) => Ok(Some(hydrate_user(db, user, UserView::General).await?)),
        None => Ok(None),
    }
}

pub async fn get_users(
    db: &DatabaseConnection,
    offset: i64,
    limit: i64,
    deleted_filter: UserDeletedFilter,
) -> Result<(Vec<UserResponse>, i64), DbErr> {
    let query = users::Entity::find().filter(match deleted_filter {
        UserDeletedFilter::Active => Condition::all().add(users::Column::DeletedAt.is_null()),
        UserDeletedFilter::Only => Condition::all().add(users::Column::DeletedAt.is_not_null()),
        UserDeletedFilter::All => Condition::all(),
    });
    let total = query.clone().count(db).await? as i64;
    let user_models = query
        .order_by_desc(users::Column::CreatedAt)
        .limit(limit.max(0) as u64)
        .offset(offset.max(0) as u64)
        .all(db)
        .await?;

    let responses = user_models
        .into_iter()
        .map(|user| UserResponse::from_entity_with_view(user, None, None, UserView::Admin))
        .collect();

    Ok((responses, total))
}

pub async fn soft_delete(db: &DatabaseConnection, id: Uuid) -> Result<bool, DbErr> {
    let Some(user) = users::Entity::find_by_id(id)
        .filter(users::Column::DeletedAt.is_null())
        .one(db)
        .await?
    else {
        return Ok(false);
    };

    let mut active = user.into_active_model();
    active.deleted_at = Set(Some(Utc::now().into()));
    active.updated_at = Set(Some(Utc::now().into()));
    active.update(db).await?;

    Ok(true)
}

#[derive(Debug)]
pub enum RestoreError {
    Db(DbErr),
    NotFound,
    Conflict,
}

impl From<DbErr> for RestoreError {
    fn from(err: DbErr) -> Self {
        Self::Db(err)
    }
}

/// Restores a soft-deleted user, mirroring echobackend's `RestoreByID`
/// (`internal/repository/user_repository.go`): rejects the restore with a
/// conflict if another user (deleted or not) already holds the same email
/// or username.
pub async fn restore(db: &DatabaseConnection, id: Uuid) -> Result<UserResponse, RestoreError> {
    let Some(user) = users::Entity::find_by_id(id)
        .filter(users::Column::DeletedAt.is_not_null())
        .one(db)
        .await?
    else {
        return Err(RestoreError::NotFound);
    };

    let email_taken = users::Entity::find()
        .filter(users::Column::Email.eq(user.email.clone()))
        .filter(users::Column::Id.ne(id))
        .one(db)
        .await?
        .is_some();
    if email_taken {
        return Err(RestoreError::Conflict);
    }

    if let Some(username) = user.username.clone().filter(|u| !u.is_empty()) {
        let username_taken = users::Entity::find()
            .filter(users::Column::Username.eq(username))
            .filter(users::Column::Id.ne(id))
            .one(db)
            .await?
            .is_some();
        if username_taken {
            return Err(RestoreError::Conflict);
        }
    }

    let mut active = user.into_active_model();
    active.deleted_at = Set(None);
    active.updated_at = Set(Some(Utc::now().into()));
    let restored_user = active.update(db).await?;

    Ok(hydrate_user(db, restored_user, UserView::Admin).await?)
}

#[derive(Debug)]
pub enum UserError {
    Db(DbErr),
    NotFound,
    UserExists,
    InvalidData(String),
}

impl From<DbErr> for UserError {
    fn from(err: DbErr) -> Self {
        Self::Db(err)
    }
}

pub async fn create_user(
    db: &DatabaseConnection,
    req: crate::dto::user::CreateUserRequest,
) -> Result<UserResponse, UserError> {
    let email_exists = users::Entity::find()
        .filter(users::Column::Email.eq(&req.email))
        .one(db)
        .await?
        .is_some();
    if email_exists {
        return Err(UserError::UserExists);
    }

    let username_exists = users::Entity::find()
        .filter(users::Column::Username.eq(&req.username))
        .one(db)
        .await?
        .is_some();
    if username_exists {
        return Err(UserError::UserExists);
    }

    let hashed = crate::services::auth::hash_password(&req.password)
        .map_err(|e| UserError::InvalidData(format!("{e:?}")))?;

    let new_user = users::ActiveModel {
        id: Set(Uuid::now_v7()),
        email: Set(req.email),
        username: Set(Some(req.username)),
        password: Set(Some(hashed)),
        first_name: Set(req.first_name),
        last_name: Set(req.last_name),
        created_at: Set(Some(Utc::now().into())),
        updated_at: Set(Some(Utc::now().into())),
        ..Default::default()
    }
    .insert(db)
    .await?;

    Ok(hydrate_user(db, new_user, UserView::Admin).await?)
}

pub async fn update_user(
    db: &DatabaseConnection,
    id: Uuid,
    req: crate::dto::user::UpdateUserRequest,
) -> Result<UserResponse, UserError> {
    let user = users::Entity::find_by_id(id)
        .filter(users::Column::DeletedAt.is_null())
        .one(db)
        .await?
        .ok_or(UserError::NotFound)?;

    if user.email != req.email {
        let email_taken = users::Entity::find()
            .filter(users::Column::Email.eq(&req.email))
            .filter(users::Column::Id.ne(id))
            .one(db)
            .await?
            .is_some();
        if email_taken {
            return Err(UserError::UserExists);
        }
    }

    if user.username.as_deref() != Some(&req.username) {
        let username_taken = users::Entity::find()
            .filter(users::Column::Username.eq(&req.username))
            .filter(users::Column::Id.ne(id))
            .one(db)
            .await?
            .is_some();
        if username_taken {
            return Err(UserError::UserExists);
        }
    }

    let mut active = user.into_active_model();
    active.email = Set(req.email);
    active.username = Set(Some(req.username));
    active.first_name = Set(req.first_name);
    active.last_name = Set(req.last_name);
    if let Some(is_super_admin) = req.is_super_admin {
        active.is_super_admin = Set(Some(is_super_admin));
    }
    active.updated_at = Set(Some(Utc::now().into()));
    let updated = active.update(db).await?;

    Ok(hydrate_user(db, updated, UserView::Admin).await?)
}
