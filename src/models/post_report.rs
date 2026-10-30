use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
pub struct PostReportGroup {
    pub post_id: Uuid,
    pub post_title: Option<String>,
    pub post_slug: Option<String>,
    pub author_username: Option<String>,
    pub hidden_at: Option<String>,
    pub report_count: i64,
    pub top_reason: String,
    pub last_reported_at: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct UserBrief {
    pub id: Uuid,
    pub username: Option<String>,
    pub image: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct ModerationActionResponse {
    pub id: Uuid,
    pub post_id: Option<Uuid>,
    pub action: String,
    pub note: Option<String>,
    pub created_at: String,
    /// echobackend `admin,omitempty`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub admin: Option<UserBrief>,
}

#[derive(Serialize, Deserialize)]
pub struct SinglePostReportResponse {
    pub id: Uuid,
    pub post_id: Uuid,
    pub reason: String,
    pub details: Option<String>,
    pub status: String,
    pub created_at: String,
    /// echobackend `reporter,omitempty` (absent for a soft-deleted reporter).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reporter: Option<UserBrief>,
    /// echobackend `resolution,omitempty`: absent while pending.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolution: Option<ModerationActionResponse>,
}
