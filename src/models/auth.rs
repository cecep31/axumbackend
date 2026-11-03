use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The `user` object inside an auth token response.
#[derive(Serialize, Deserialize)]
pub struct UserBrief {
    pub id: Uuid,
    pub email: String,
    pub username: Option<String>,
}

/// `POST /api/auth/check-username` payload.
#[derive(Debug, Serialize)]
pub struct CheckUsernameResponse {
    pub exists: bool,
}

/// `DELETE /api/auth/sessions/{user_id}` payload; `user_id` echoes the raw
/// path value.
#[derive(Debug, Serialize)]
pub struct RevokeUserSessionsResponse {
    pub user_id: String,
    pub revoked_sessions: u64,
}

/// `POST /api/auth/sessions/revoke-all` payload.
#[derive(Debug, Serialize)]
pub struct RevokeAllSessionsResponse {
    pub revoked_sessions: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn check_username_response_shape() {
        let exists = true;
        assert_eq!(
            serde_json::to_value(CheckUsernameResponse { exists }).unwrap(),
            json!({ "exists": exists })
        );
    }

    #[test]
    fn revoke_user_sessions_response_shape() {
        let user_id = "6f1c2a3b-4d5e-4f60-8a7b-9c0d1e2f3a4b".to_string();
        let revoked: u64 = 3;
        assert_eq!(
            serde_json::to_value(RevokeUserSessionsResponse {
                user_id: user_id.clone(),
                revoked_sessions: revoked,
            })
            .unwrap(),
            json!({ "user_id": user_id, "revoked_sessions": revoked })
        );
    }

    #[test]
    fn revoke_all_sessions_response_shape() {
        let revoked: u64 = 42;
        assert_eq!(
            serde_json::to_value(RevokeAllSessionsResponse {
                revoked_sessions: revoked,
            })
            .unwrap(),
            json!({ "revoked_sessions": revoked })
        );
    }

    #[test]
    fn user_brief_shape() {
        let id = Uuid::nil();
        assert_eq!(
            serde_json::to_value(UserBrief {
                id,
                email: "a@b.c".into(),
                username: None,
            })
            .unwrap(),
            json!({ "id": id, "email": "a@b.c", "username": null })
        );
    }
}
