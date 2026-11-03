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

/// `data` of `GET /api/users/{id}/follow-status`.
#[derive(Serialize)]
pub struct FollowStatusResponse {
    pub is_following: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follow_status_response_matches_old_json() {
        for is_following in [true, false] {
            assert_eq!(
                serde_json::to_value(FollowStatusResponse { is_following }).unwrap(),
                serde_json::json!({ "is_following": is_following })
            );
        }
    }
}
