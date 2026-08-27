use super::user::User;
use chrono::{DateTime, FixedOffset, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Serialize)]
pub struct PostLikeResponse {
    pub id: Uuid,
    pub post_id: Uuid,
    pub user_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<User>,
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
pub struct PostLikeStats {
    pub post_id: Uuid,
    pub total_likes: i64,
}

#[derive(Serialize)]
pub struct PostLikeListResponse {
    pub likes: Vec<PostLikeResponse>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Serialize)]
pub struct LikeStatusResponse {
    pub has_liked: bool,
    pub post_id: Uuid,
    pub user_id: Uuid,
}

fn to_utc(value: Option<DateTime<FixedOffset>>) -> Option<DateTime<Utc>> {
    value.map(|dt| dt.with_timezone(&Utc))
}

impl PostLikeResponse {
    pub fn from_entity(like: crate::entities::post_likes::Model, user: Option<User>) -> Self {
        Self {
            id: like.id,
            post_id: like.post_id,
            user_id: like.user_id,
            user,
            created_at: to_utc(like.created_at),
        }
    }
}
