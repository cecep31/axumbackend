use crate::entities::users;
use crate::models::user::{UserResponse, UserView};
use crate::services::user_hydration;
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, IntoActiveModel,
    PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, Set,
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
/// info (echobackend `UserToAdminResponse`).
pub async fn get_admin_by_id(
    db: &DatabaseConnection,
    id: Uuid,
) -> Result<Option<UserResponse>, DbErr> {
    let user = users::Entity::find_by_id(id)
        .filter(users::Column::DeletedAt.is_null())
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
) -> Result<(Vec<UserResponse>, i64), DbErr> {
    let query = users::Entity::find().filter(users::Column::DeletedAt.is_null());
    let total = query.clone().count(db).await? as i64;
    let user_models = query
        .order_by_desc(users::Column::CreatedAt)
        .limit(limit.max(0) as u64)
        .offset(offset.max(0) as u64)
        .all(db)
        .await?;

    let users_by_id = user_hydration::load_user_response_map(
        db,
        user_models.iter().map(|user| user.id),
        UserView::Admin,
    )
    .await?;
    let responses = user_models
        .into_iter()
        .map(|user| {
            users_by_id.get(&user.id).cloned().unwrap_or_else(|| {
                UserResponse::from_entity_with_view(user, None, None, UserView::Admin)
            })
        })
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

pub async fn restore(db: &DatabaseConnection, id: Uuid) -> Result<bool, DbErr> {
    let Some(user) = users::Entity::find_by_id(id)
        .filter(users::Column::DeletedAt.is_not_null())
        .one(db)
        .await?
    else {
        return Ok(false);
    };

    let mut active = user.into_active_model();
    active.deleted_at = Set(None);
    active.updated_at = Set(Some(Utc::now().into()));
    active.update(db).await?;

    Ok(true)
}
