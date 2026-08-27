use super::user::User;
use chrono::{DateTime, FixedOffset, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Serialize)]
pub struct CommentResponse {
    pub id: Uuid,
    pub post_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_comment_id: Option<Uuid>,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<User>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

fn to_utc(value: Option<DateTime<FixedOffset>>) -> Option<DateTime<Utc>> {
    value.map(|dt| dt.with_timezone(&Utc))
}

impl CommentResponse {
    pub fn from_entity(comment: crate::entities::post_comments::Model, user: Option<User>) -> Self {
        Self {
            id: comment.id,
            post_id: comment.post_id,
            parent_comment_id: comment.parent_comment_id,
            text: comment.text,
            user,
            created_at: to_utc(comment.created_at),
            updated_at: to_utc(comment.updated_at),
        }
    }
}
