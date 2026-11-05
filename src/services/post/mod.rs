//! Post queries and writes. `read` holds the list/detail queries, `write` the
//! create/update/delete paths and tag syncing; the shared active-author
//! helpers live here.

mod read;
mod write;

pub use read::*;
pub use write::*;

use crate::entities::{posts, users};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DbErr, EntityTrait, JoinType, QueryFilter, QuerySelect,
    RelationTrait, Select,
};

/// drops posts whose author is soft-deleted
/// (`JOIN users ON users.id = posts.created_by AND users.deleted_at IS NULL`).
pub fn active_posts() -> Select<posts::Entity> {
    posts::Entity::find()
        .join(JoinType::InnerJoin, posts::Relation::Users.def())
        .filter(users::Column::DeletedAt.is_null())
}

/// echobackend's `GetPostByID`: the post (drafts and hidden posts included)
/// unless it is missing or its author is soft-deleted.
pub async fn find_active_post(
    db: &impl ConnectionTrait,
    post_id: uuid::Uuid,
) -> Result<Option<posts::Model>, DbErr> {
    active_posts()
        .filter(posts::Column::Id.eq(post_id))
        .one(db)
        .await
}

/// echobackend's `GetPostByID` + `ensurePostInteractable`, keeping apart the
/// two ways it fails: the lookup itself (`GetPostByID` -> `ErrPostNotFound`,
/// wrapped by the caller, e.g. "failed to check post existence: post not
/// found") and a hidden post (`ensurePostInteractable` -> bare
/// `ErrPostNotFound`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostPresence {
    /// Missing, or its author is soft-deleted.
    Missing,
    /// Exists but `hidden_at` is set by a moderator.
    Hidden,
    Interactable,
}

pub async fn post_presence(
    db: &impl ConnectionTrait,
    post_id: uuid::Uuid,
) -> Result<PostPresence, DbErr> {
    Ok(match find_active_post(db, post_id).await? {
        None => PostPresence::Missing,
        Some(post) if post.hidden_at.is_some() => PostPresence::Hidden,
        Some(_) => PostPresence::Interactable,
    })
}

/// echobackend's `GetPostByID` + `ensurePostInteractable`: whether a post can
/// receive engagement (like, comment, bookmark, view, report). A hidden post
/// or one whose author is soft-deleted is reported as missing.
pub async fn interactable_post_exists(
    db: &impl ConnectionTrait,
    post_id: uuid::Uuid,
) -> Result<bool, DbErr> {
    Ok(active_posts()
        .filter(posts::Column::Id.eq(post_id))
        .filter(posts::Column::HiddenAt.is_null())
        .one(db)
        .await?
        .is_some())
}
