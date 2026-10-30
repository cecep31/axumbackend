use crate::entities::post_likes;
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

/// echobackend `LikePost`: `GetPostByID` + `ensurePostInteractable`, so a
/// hidden post or one whose author is soft-deleted is reported as missing.
async fn post_interactable(db: &DatabaseConnection, post_id: Uuid) -> Result<bool, DbErr> {
    crate::services::post::interactable_post_exists(db, post_id).await
}

/// echobackend `UnlikePost`: only `GetPostByID` (author not soft-deleted).
/// Hidden posts pass, so users can always undo a like on a moderated post.
async fn post_active(db: &DatabaseConnection, post_id: Uuid) -> Result<bool, DbErr> {
    Ok(crate::services::post::find_active_post(db, post_id)
        .await?
        .is_some())
}

async fn like_exists(db: &DatabaseConnection, post_id: Uuid, user_id: Uuid) -> Result<bool, DbErr> {
    Ok(post_likes::Entity::find()
        .filter(post_likes::Column::PostId.eq(post_id))
        .filter(post_likes::Column::UserId.eq(user_id))
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
    if !post_interactable(db, post_id).await? {
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
    if !post_active(db, post_id).await? {
        return Err(PostLikeError::PostNotFound);
    }

    let result = post_likes::Entity::delete_many()
        .filter(post_likes::Column::PostId.eq(post_id))
        .filter(post_likes::Column::UserId.eq(user_id))
        .exec(db)
        .await?;

    if result.rows_affected == 0 {
        return Err(PostLikeError::NotLiked);
    }

    Ok(())
}

/// Like echobackend's read endpoints (`GetLikesByPostID`, `GetLikeStats`,
/// `HasUserLikedPost`), the post itself is not looked up: a missing or hidden
/// post simply has no likes.
pub async fn get_likes_by_post_id(
    db: &DatabaseConnection,
    post_id: Uuid,
    limit: i64,
    offset: i64,
) -> Result<PostLikeListResponse, PostLikeError> {
    let query = post_likes::Entity::find().filter(post_likes::Column::PostId.eq(post_id));

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
    let total_likes = post_likes::Entity::find()
        .filter(post_likes::Column::PostId.eq(post_id))
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
    Ok(LikeStatusResponse {
        has_liked: like_exists(db, post_id, user_id).await?,
        post_id,
        user_id,
    })
}
