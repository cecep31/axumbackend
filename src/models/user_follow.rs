use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
pub struct FollowResponse {
    pub is_following: bool,
    pub message: String,
}

#[derive(Serialize, Deserialize)]
pub struct FollowStats {
    pub user_id: Uuid,
    pub followers_count: i64,
    pub following_count: i64,
}
