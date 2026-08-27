use crate::entities::{profiles, user_follows, users};
use crate::models::user::UserResponse;
use crate::models::user_follow::{FollowResponse, FollowStats};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, JoinType,
    PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, QueryTrait, RelationTrait, Set,
};
use uuid::Uuid;

#[derive(Debug)]
pub enum UserFollowError {
    Db(DbErr),
    UserNotFound,
    CannotFollowSelf,
    AlreadyFollowing,
    NotFollowing,
}

impl From<DbErr> for UserFollowError {
    fn from(err: DbErr) -> Self {
        Self::Db(err)
    }
}

async fn user_exists(db: &DatabaseConnection, id: Uuid) -> Result<bool, DbErr> {
    users::Entity::find_by_id(id)
        .filter(users::Column::DeletedAt.is_null())
        .one(db)
        .await
        .map(|user| user.is_some())
}

pub async fn is_following(
    db: &DatabaseConnection,
    follower_id: Uuid,
    following_id: Uuid,
) -> Result<bool, DbErr> {
    user_follows::Entity::find()
        .filter(user_follows::Column::FollowerId.eq(follower_id))
        .filter(user_follows::Column::FollowingId.eq(following_id))
        .filter(user_follows::Column::DeletedAt.is_null())
        .one(db)
        .await
        .map(|follow| follow.is_some())
}

pub async fn follow_user(
    db: &DatabaseConnection,
    follower_id: Uuid,
    following_id: Uuid,
) -> Result<FollowResponse, UserFollowError> {
    if follower_id == following_id {
        return Err(UserFollowError::CannotFollowSelf);
    }

    if !user_exists(db, follower_id).await? || !user_exists(db, following_id).await? {
        return Err(UserFollowError::UserNotFound);
    }

    if is_following(db, follower_id, following_id).await? {
        return Err(UserFollowError::AlreadyFollowing);
    }

    user_follows::ActiveModel {
        id: Set(Uuid::now_v7()),
        follower_id: Set(follower_id),
        following_id: Set(following_id),
        created_at: Set(Some(Utc::now().into())),
        updated_at: Set(Some(Utc::now().into())),
        deleted_at: Set(None),
    }
    .insert(db)
    .await?;

    Ok(FollowResponse {
        is_following: true,
        message: "Successfully followed user".to_string(),
    })
}

pub async fn unfollow_user(
    db: &DatabaseConnection,
    follower_id: Uuid,
    following_id: Uuid,
) -> Result<FollowResponse, UserFollowError> {
    let Some(follow) = user_follows::Entity::find()
        .filter(user_follows::Column::FollowerId.eq(follower_id))
        .filter(user_follows::Column::FollowingId.eq(following_id))
        .filter(user_follows::Column::DeletedAt.is_null())
        .one(db)
        .await?
    else {
        return Err(UserFollowError::NotFollowing);
    };

    let mut active: user_follows::ActiveModel = follow.into();
    active.deleted_at = Set(Some(Utc::now().into()));
    active.updated_at = Set(Some(Utc::now().into()));
    active.update(db).await?;

    Ok(FollowResponse {
        is_following: false,
        message: "Successfully unfollowed user".to_string(),
    })
}

async fn user_response(
    db: &DatabaseConnection,
    user: users::Model,
    current_user_id: Option<Uuid>,
) -> Result<UserResponse, DbErr> {
    let profile = profiles::Entity::find()
        .filter(profiles::Column::UserId.eq(user.id))
        .one(db)
        .await?;
    let is_following = match current_user_id {
        Some(current_user_id) if current_user_id != user.id => {
            Some(is_following(db, current_user_id, user.id).await?)
        }
        _ => None,
    };
    Ok(UserResponse::from_entity(user, profile, is_following))
}

pub async fn get_followers(
    db: &DatabaseConnection,
    user_id: Uuid,
    limit: i64,
    offset: i64,
    current_user_id: Option<Uuid>,
) -> Result<(Vec<UserResponse>, i64), DbErr> {
    let query = users::Entity::find()
        .join(
            JoinType::InnerJoin,
            user_follows::Relation::Follower.def().rev(),
        )
        .filter(user_follows::Column::FollowingId.eq(user_id))
        .filter(user_follows::Column::DeletedAt.is_null())
        .filter(users::Column::DeletedAt.is_null());

    let total = query.clone().count(db).await? as i64;
    let users = query
        .order_by_desc(user_follows::Column::CreatedAt)
        .limit(limit.max(0) as u64)
        .offset(offset.max(0) as u64)
        .all(db)
        .await?;

    let mut responses = Vec::with_capacity(users.len());
    for user in users {
        responses.push(user_response(db, user, current_user_id).await?);
    }

    Ok((responses, total))
}

pub async fn get_following(
    db: &DatabaseConnection,
    user_id: Uuid,
    limit: i64,
    offset: i64,
    current_user_id: Option<Uuid>,
) -> Result<(Vec<UserResponse>, i64), DbErr> {
    let query = users::Entity::find()
        .join(
            JoinType::InnerJoin,
            user_follows::Relation::Following.def().rev(),
        )
        .filter(user_follows::Column::FollowerId.eq(user_id))
        .filter(user_follows::Column::DeletedAt.is_null())
        .filter(users::Column::DeletedAt.is_null());

    let total = query.clone().count(db).await? as i64;
    let users = query
        .order_by_desc(user_follows::Column::CreatedAt)
        .limit(limit.max(0) as u64)
        .offset(offset.max(0) as u64)
        .all(db)
        .await?;

    let mut responses = Vec::with_capacity(users.len());
    for user in users {
        responses.push(user_response(db, user, current_user_id).await?);
    }

    Ok((responses, total))
}

pub async fn get_follow_stats(
    db: &DatabaseConnection,
    user_id: Uuid,
) -> Result<Option<FollowStats>, DbErr> {
    let Some(user) = users::Entity::find_by_id(user_id)
        .filter(users::Column::DeletedAt.is_null())
        .one(db)
        .await?
    else {
        return Ok(None);
    };

    Ok(Some(FollowStats {
        user_id,
        followers_count: user.followers_count.unwrap_or_default(),
        following_count: user.following_count.unwrap_or_default(),
    }))
}

/// Mirrors echobackend's `GetMutualFollows`, which builds entries via plain
/// `UserToResponse` — `is_following` is never populated for this endpoint.
pub async fn get_mutual_follows(
    db: &DatabaseConnection,
    user_id: Uuid,
    other_user_id: Uuid,
) -> Result<Vec<UserResponse>, DbErr> {
    let user_following = user_follows::Entity::find()
        .select_only()
        .column(user_follows::Column::FollowingId)
        .filter(user_follows::Column::FollowerId.eq(user_id))
        .filter(user_follows::Column::DeletedAt.is_null());

    let users = users::Entity::find()
        .join(
            JoinType::InnerJoin,
            user_follows::Relation::Following.def().rev(),
        )
        .filter(user_follows::Column::FollowerId.eq(other_user_id))
        .filter(user_follows::Column::DeletedAt.is_null())
        .filter(users::Column::Id.in_subquery(user_following.into_query()))
        .filter(users::Column::DeletedAt.is_null())
        .order_by_asc(users::Column::Username)
        .all(db)
        .await?;

    let mut responses = Vec::with_capacity(users.len());
    for user in users {
        responses.push(user_response(db, user, None).await?);
    }

    Ok(responses)
}
