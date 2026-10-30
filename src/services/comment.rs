use crate::entities::{post_comments, posts};
use crate::models::comment::CommentResponse;
use crate::models::user::User;
use crate::services::user_hydration;
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, IntoActiveModel,
    QueryFilter, QueryOrder, Set,
};
use uuid::Uuid;

#[derive(Debug)]
pub enum CommentError {
    Db(DbErr),
    PostNotFound,
    CommentNotFound,
    NotOwner,
    /// Text tripped the anti-spam guard (`crate::domain::antispam`); carries the
    /// user-facing reason surfaced through the 422 envelope.
    SpamRejected(String),
}

/// Runs before any database access so rejected text never costs a query.
fn reject_spam(text: &str) -> Result<(), CommentError> {
    match crate::domain::antispam::text_reject_reason(&[text]) {
        Some(reason) => Err(CommentError::SpamRejected(reason)),
        None => Ok(()),
    }
}

impl From<DbErr> for CommentError {
    fn from(err: DbErr) -> Self {
        Self::Db(err)
    }
}

/// echobackend `GetPostByID` + `ensurePostInteractable`: the post, unless it
/// is missing, hidden or written by a soft-deleted author (all reported as
/// "post not found" so engagement endpoints do not confirm it exists).
async fn interactable_post(
    db: &DatabaseConnection,
    post_id: Uuid,
) -> Result<Option<posts::Model>, DbErr> {
    Ok(crate::services::post::find_active_post(db, post_id)
        .await?
        .filter(|post| post.hidden_at.is_none()))
}

/// `notifications.data` for a new comment. Keys are in alphabetical order,
/// matching Go's map marshaling in echobackend.
fn comment_notification_data(actor_id: Uuid, comment_id: Uuid, post_id: Uuid) -> serde_json::Value {
    serde_json::json!({
        "actor_id": actor_id,
        "comment_id": comment_id,
        "post_id": post_id,
    })
}

/// echobackend `CreateComment`: tells the post author someone commented.
/// Best-effort, like echobackend (`_, _ = CreateNotification(..)`): a failure
/// is logged and never fails the comment.
async fn notify_post_author(
    db: &DatabaseConnection,
    post: &posts::Model,
    comment: &post_comments::Model,
) {
    if post.created_by == comment.created_by {
        return;
    }
    let data = comment_notification_data(comment.created_by, comment.id, post.id);
    if let Err(err) = crate::services::notification::create_notification(
        db,
        post.created_by,
        "comment",
        "New comment",
        Some("Someone commented on your post".to_string()),
        Some(data),
    )
    .await
    {
        tracing::warn!(error = %err, post_id = %post.id, "failed to create comment notification");
    }
}

fn hydrate_comment(
    comment: post_comments::Model,
    users_by_id: &std::collections::HashMap<Uuid, User>,
) -> CommentResponse {
    let user = users_by_id.get(&comment.created_by).cloned();
    CommentResponse::from_entity(comment, user)
}

async fn load_comment_user_map(
    db: &DatabaseConnection,
    comments: &[post_comments::Model],
) -> Result<std::collections::HashMap<Uuid, User>, DbErr> {
    user_hydration::load_user_brief_map(db, comments.iter().map(|comment| comment.created_by)).await
}

pub async fn create_comment(
    db: &DatabaseConnection,
    post_id: Uuid,
    text: String,
    created_by: Uuid,
) -> Result<CommentResponse, CommentError> {
    reject_spam(&text)?;
    let Some(post) = interactable_post(db, post_id).await? else {
        return Err(CommentError::PostNotFound);
    };

    let now = Utc::now();
    let comment = post_comments::ActiveModel {
        id: Set(Uuid::now_v7()),
        post_id: Set(post_id),
        text: Set(text),
        created_by: Set(created_by),
        parent_comment_id: Set(None),
        created_at: Set(Some(now.into())),
        updated_at: Set(Some(now.into())),
        deleted_at: Set(None),
    }
    .insert(db)
    .await?;

    let users_by_id = load_comment_user_map(db, std::slice::from_ref(&comment)).await?;
    notify_post_author(db, &post, &comment).await;
    Ok(hydrate_comment(comment, &users_by_id))
}

pub async fn get_comments_by_post_id(
    db: &DatabaseConnection,
    post_id: Uuid,
) -> Result<Vec<CommentResponse>, CommentError> {
    if interactable_post(db, post_id).await?.is_none() {
        return Err(CommentError::PostNotFound);
    }

    let comments = post_comments::Entity::find()
        .filter(post_comments::Column::PostId.eq(post_id))
        .filter(post_comments::Column::DeletedAt.is_null())
        .order_by_desc(post_comments::Column::CreatedAt)
        .all(db)
        .await?;

    let users_by_id = load_comment_user_map(db, &comments).await?;
    let responses = comments
        .into_iter()
        .map(|comment| hydrate_comment(comment, &users_by_id))
        .collect();
    Ok(responses)
}

pub async fn update_comment(
    db: &DatabaseConnection,
    post_id: Uuid,
    comment_id: Uuid,
    text: String,
    user_id: Uuid,
) -> Result<CommentResponse, CommentError> {
    reject_spam(&text)?;
    let Some(comment) = post_comments::Entity::find_by_id(comment_id)
        .filter(post_comments::Column::PostId.eq(post_id))
        .filter(post_comments::Column::DeletedAt.is_null())
        .one(db)
        .await?
    else {
        return Err(CommentError::CommentNotFound);
    };

    if comment.created_by != user_id {
        return Err(CommentError::NotOwner);
    }

    let mut active = comment.into_active_model();
    active.text = Set(text);
    active.updated_at = Set(Some(Utc::now().into()));
    let updated = active.update(db).await?;

    let users_by_id = load_comment_user_map(db, std::slice::from_ref(&updated)).await?;
    Ok(hydrate_comment(updated, &users_by_id))
}

/// Soft-deletes a comment. Only its author may delete it, unless `is_admin`
/// (a DB-verified super admin, echobackend `DeleteComment(.., isAdmin)`).
pub async fn delete_comment(
    db: &DatabaseConnection,
    post_id: Uuid,
    comment_id: Uuid,
    user_id: Uuid,
    is_admin: bool,
) -> Result<(), CommentError> {
    let Some(comment) = post_comments::Entity::find_by_id(comment_id)
        .filter(post_comments::Column::PostId.eq(post_id))
        .filter(post_comments::Column::DeletedAt.is_null())
        .one(db)
        .await?
    else {
        return Err(CommentError::CommentNotFound);
    };

    if !is_admin && comment.created_by != user_id {
        return Err(CommentError::NotOwner);
    }

    let mut active = comment.into_active_model();
    active.deleted_at = Set(Some(Utc::now().into()));
    active.updated_at = Set(Some(Utc::now().into()));
    active.update(db).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_data_matches_echobackend_json() {
        let actor = Uuid::parse_str("0190a000-0000-7000-8000-000000000001").unwrap();
        let comment = Uuid::parse_str("0190a000-0000-7000-8000-000000000002").unwrap();
        let post = Uuid::parse_str("0190a000-0000-7000-8000-000000000003").unwrap();
        assert_eq!(
            comment_notification_data(actor, comment, post).to_string(),
            "{\"actor_id\":\"0190a000-0000-7000-8000-000000000001\",\
             \"comment_id\":\"0190a000-0000-7000-8000-000000000002\",\
             \"post_id\":\"0190a000-0000-7000-8000-000000000003\"}"
        );
    }
}
