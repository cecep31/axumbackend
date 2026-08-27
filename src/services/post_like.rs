use crate::entities::{post_likes, posts};
use crate::models::post_like::{
    LikeStatusResponse, PostLikeListResponse, PostLikeResponse, PostLikeStats,
};
use crate::models::user::User;
use crate::services::user_hydration;
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, PaginatorTrait,
    QueryFilter, QueryOrder, QuerySelect, Set,
};
use uuid::Uuid;

#[derive(Debug)]
pub enum PostLikeError {
    Db(DbErr),
    PostNotFound,
    AlreadyLiked,
    NotLiked,
}

impl From<DbErr> for PostLikeError {
    fn from(err: DbErr) -> Self {
        Self::Db(err)
    }
}

async fn post_exists(db: &DatabaseConnection, post_id: Uuid) -> Result<bool, DbErr> {
    Ok(posts::Entity::find_by_id(post_id)
        .filter(posts::Column::DeletedAt.is_null())
        .one(db)
        .await?
        .is_some())
}

async fn like_exists(db: &DatabaseConnection, post_id: Uuid, user_id: Uuid) -> Result<bool, DbErr> {
    Ok(post_likes::Entity::find()
        .filter(post_likes::Column::PostId.eq(post_id))
        .filter(post_likes::Column::UserId.eq(user_id))
        .filter(post_likes::Column::DeletedAt.is_null())
        .one(db)
        .await?
        .is_some())
}

fn hydrate_like(
    like: post_likes::Model,
    users_by_id: &std::collections::HashMap<Uuid, User>,
) -> PostLikeResponse {
    let user = users_by_id.get(&like.user_id).cloned();
    PostLikeResponse::from_entity(like, user)
}

async fn load_like_user_map(
    db: &DatabaseConnection,
    likes: &[post_likes::Model],
) -> Result<std::collections::HashMap<Uuid, User>, DbErr> {
    user_hydration::load_user_brief_map(db, likes.iter().map(|like| like.user_id)).await
}

pub async fn like_post(
    db: &DatabaseConnection,
    post_id: Uuid,
    user_id: Uuid,
) -> Result<(), PostLikeError> {
    if !post_exists(db, post_id).await? {
        return Err(PostLikeError::PostNotFound);
    }
    if like_exists(db, post_id, user_id).await? {
        return Err(PostLikeError::AlreadyLiked);
    }

    let now = Utc::now();
    post_likes::ActiveModel {
        id: Set(Uuid::now_v7()),
        post_id: Set(post_id),
        user_id: Set(user_id),
        created_at: Set(Some(now.into())),
        updated_at: Set(Some(now.into())),
        deleted_at: Set(None),
    }
    .insert(db)
    .await?;

    Ok(())
}

pub async fn unlike_post(
    db: &DatabaseConnection,
    post_id: Uuid,
    user_id: Uuid,
) -> Result<(), PostLikeError> {
    if !post_exists(db, post_id).await? {
        return Err(PostLikeError::PostNotFound);
    }

    let result = post_likes::Entity::delete_many()
        .filter(post_likes::Column::PostId.eq(post_id))
        .filter(post_likes::Column::UserId.eq(user_id))
        .filter(post_likes::Column::DeletedAt.is_null())
        .exec(db)
        .await?;

    if result.rows_affected == 0 {
        return Err(PostLikeError::NotLiked);
    }

    Ok(())
}

pub async fn get_likes_by_post_id(
    db: &DatabaseConnection,
    post_id: Uuid,
    limit: i64,
    offset: i64,
) -> Result<PostLikeListResponse, PostLikeError> {
    if !post_exists(db, post_id).await? {
        return Err(PostLikeError::PostNotFound);
    }

    let query = post_likes::Entity::find()
        .filter(post_likes::Column::PostId.eq(post_id))
        .filter(post_likes::Column::DeletedAt.is_null());

    let total = query.clone().count(db).await? as i64;
    let like_models = query
        .order_by_desc(post_likes::Column::CreatedAt)
        .limit(limit.max(0) as u64)
        .offset(offset.max(0) as u64)
        .all(db)
        .await?;

    let users_by_id = load_like_user_map(db, &like_models).await?;
    let likes = like_models
        .into_iter()
        .map(|like| hydrate_like(like, &users_by_id))
        .collect();

    Ok(PostLikeListResponse {
        likes,
        total,
        limit,
        offset,
    })
}

pub async fn get_like_stats(
    db: &DatabaseConnection,
    post_id: Uuid,
) -> Result<PostLikeStats, PostLikeError> {
    if !post_exists(db, post_id).await? {
        return Err(PostLikeError::PostNotFound);
    }

    let total_likes = post_likes::Entity::find()
        .filter(post_likes::Column::PostId.eq(post_id))
        .filter(post_likes::Column::DeletedAt.is_null())
        .count(db)
        .await? as i64;

    Ok(PostLikeStats {
        post_id,
        total_likes,
    })
}

pub async fn has_user_liked_post(
    db: &DatabaseConnection,
    post_id: Uuid,
    user_id: Uuid,
) -> Result<LikeStatusResponse, PostLikeError> {
    if !post_exists(db, post_id).await? {
        return Err(PostLikeError::PostNotFound);
    }

    Ok(LikeStatusResponse {
        has_liked: like_exists(db, post_id, user_id).await?,
        post_id,
        user_id,
    })
}
