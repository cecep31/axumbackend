use super::user::UserResponse;
use chrono::{DateTime, FixedOffset, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Serialize)]
pub struct MyPostsAnalyticsResponse {
    pub total_posts: i64,
    pub total_views: i64,
    pub total_likes: i64,
    pub total_bookmarks: i64,
}

#[derive(Serialize)]
pub struct MyPostsLikesByMonthResponse {
    pub month: String,
    pub total_likes: i64,
}

#[derive(Serialize)]
pub struct PostViewResponse {
    pub id: Uuid,
    pub post_id: Uuid,
    pub user_id: Option<Uuid>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<UserResponse>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
pub struct PostViewStats {
    pub post_id: Uuid,
    pub total_views: i64,
    pub unique_views: i64,
    pub anonymous_views: i64,
    pub authenticated_views: i64,
}

#[derive(Serialize)]
pub struct ViewStatusResponse {
    pub has_viewed: bool,
}

fn to_utc(value: Option<DateTime<FixedOffset>>) -> Option<DateTime<Utc>> {
    value.map(|dt| dt.with_timezone(&Utc))
}

impl PostViewResponse {
    pub fn from_entity(
        view: crate::entities::post_views::Model,
        user: Option<UserResponse>,
    ) -> Self {
        Self {
            id: view.id,
            post_id: view.post_id,
            user_id: view.user_id,
            ip_address: view.ip_address,
            user_agent: view.user_agent,
            user,
            created_at: to_utc(view.created_at),
            updated_at: to_utc(view.updated_at),
        }
    }
}
