use chrono::{DateTime, FixedOffset, Utc};
use serde::Serialize;
use uuid::Uuid;

/// Mirrors echobackend's `dto.MyPostsAnalyticsSummary`.
#[derive(Serialize)]
pub struct MyPostsAnalyticsSummary {
    pub total_posts: i64,
    pub published_posts: i64,
    pub total_views: i64,
    pub total_likes: i64,
}

/// Mirrors echobackend's `dto.MyPostsViewTrendPoint`.
#[derive(Serialize)]
pub struct MyPostsViewTrendPoint {
    pub date: String,
    pub views: i64,
    pub cumulative_views: i64,
}

/// Mirrors echobackend's `dto.MyPostPerformance`.
#[derive(Serialize)]
pub struct MyPostPerformance {
    pub id: Uuid,
    pub title: String,
    pub slug: String,
    pub view_count: i64,
    pub like_count: i64,
}

/// Mirrors echobackend's `dto.MyPostsAnalyticsResponse`.
#[derive(Serialize)]
pub struct MyPostsAnalyticsResponse {
    pub summary: MyPostsAnalyticsSummary,
    pub view_trend: Vec<MyPostsViewTrendPoint>,
    pub top_posts: Vec<MyPostPerformance>,
}

/// Mirrors echobackend's `dto.MyPostsLikesByMonthPoint`.
#[derive(Serialize)]
pub struct MyPostsLikesByMonthPoint {
    pub month: String,
    pub likes: i64,
}

/// Mirrors echobackend's `dto.MyPostsLikesByMonthResponse`.
#[derive(Serialize)]
pub struct MyPostsLikesByMonthResponse {
    pub months: i64,
    pub series: Vec<MyPostsLikesByMonthPoint>,
    pub total: i64,
}

/// Mirrors echobackend's `dto.PostViewResponse` (no nested `user`).
#[derive(Serialize)]
pub struct PostViewResponse {
    pub id: Uuid,
    pub post_id: Uuid,
    pub user_id: Option<Uuid>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
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
    pub fn from_entity(view: crate::entities::post_views::Model) -> Self {
        Self {
            id: view.id,
            post_id: view.post_id,
            user_id: view.user_id,
            ip_address: view.ip_address,
            user_agent: view.user_agent,
            created_at: to_utc(view.created_at),
            updated_at: to_utc(view.updated_at),
        }
    }
}
