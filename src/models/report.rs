use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverviewStatsResponse {
    pub total_users: i64,
    pub total_posts: i64,
    pub total_views: i64,
    pub total_likes: i64,
    pub total_comments: i64,
    pub new_users_today: i64,
    pub new_posts_today: i64,
    pub active_users_this_week: i64,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserGrowthData {
    pub date: String,
    pub new_users: i64,
    pub cumulative_users: i64,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TopContributor {
    pub id: Uuid,
    pub username: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub post_count: i64,
    pub total_views: i64,
    pub total_likes: i64,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserReportResponse {
    pub total_users: i64,
    pub new_users_this_period: i64,
    pub active_users: i64,
    pub top_contributors: Vec<TopContributor>,
    pub growth_trend: Vec<UserGrowthData>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostPerformanceAuthor {
    pub id: Uuid,
    pub username: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostPerformanceData {
    pub id: Uuid,
    pub title: Option<String>,
    pub slug: Option<String>,
    pub views: i64,
    pub likes: i64,
    pub comments: i64,
    pub engagement_rate: f64,
    pub author: PostPerformanceAuthor,
    pub created_at: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagPerformance {
    pub id: i32,
    pub name: String,
    pub post_count: i64,
    pub total_views: i64,
    pub total_likes: i64,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostReportResponse {
    pub total_posts: i64,
    pub new_posts_this_period: i64,
    pub total_views: i64,
    pub total_likes: i64,
    pub total_comments: i64,
    pub avg_engagement_rate: f64,
    pub top_posts: Vec<PostPerformanceData>,
    pub tag_performance: Vec<TagPerformance>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeriodComparison {
    pub current: i64,
    pub previous: i64,
    pub change_percent: f64,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngagementMetricsResponse {
    pub total_engagements: i64,
    pub avg_likes_per_post: f64,
    pub avg_comments_per_post: f64,
    pub avg_views_per_post: f64,
    pub period_comparison: PeriodComparison,
}
