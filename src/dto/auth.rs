use serde::Deserialize;
use validator::Validate;

#[derive(Deserialize, Validate)]
pub struct RegisterRequest {
    #[validate(email)]
    pub email: String,
    #[validate(length(min = 3, max = 30))]
    pub username: String,
    #[validate(length(min = 8))]
    pub password: String,
}

#[derive(Deserialize, Validate)]
pub struct LoginRequest {
    #[validate(length(min = 1))]
    pub identifier: String,
    #[validate(length(min = 6))]
    pub password: String,
}

#[derive(Deserialize, Validate)]
pub struct RefreshTokenRequest {
    #[validate(length(min = 1))]
    pub refresh_token: String,
}

#[derive(Deserialize, Validate)]
pub struct ForgotPasswordRequest {
    #[validate(email)]
    pub email: String,
}

#[derive(Deserialize, Validate)]
pub struct ResetPasswordRequest {
    #[validate(length(min = 1))]
    pub token: String,
    #[validate(length(min = 8))]
    pub password: String,
}

#[derive(Deserialize, Validate)]
pub struct ChangePasswordRequest {
    #[validate(length(min = 8))]
    pub current_password: String,
    #[validate(length(min = 8))]
    pub new_password: String,
}

#[derive(Deserialize, Validate)]
pub struct LogoutRequest {
    #[validate(length(min = 1))]
    pub refresh_token: String,
}

#[derive(Deserialize, Validate)]
pub struct OAuthExchangeRequest {
    #[validate(length(min = 1))]
    pub code: String,
}

#[derive(Debug, Deserialize)]
pub struct GithubCallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
}

#[derive(Deserialize, Validate)]
pub struct ActivityLogQuery {
    #[validate(range(min = 0, max = 10_000))]
    pub offset: Option<i64>,
    #[validate(range(min = 1, max = 100))]
    pub limit: Option<i64>,
    #[validate(length(min = 1, max = 50))]
    pub activity_type: Option<String>,
}

#[derive(Deserialize, Validate)]
pub struct RecentActivityQuery {
    #[validate(range(min = 1, max = 50))]
    pub limit: Option<i64>,
}

#[derive(Deserialize, Validate)]
pub struct FailedLoginsQuery {
    #[validate(range(min = 0, max = 10_000))]
    pub offset: Option<i64>,
    #[validate(range(min = 1, max = 100))]
    pub limit: Option<i64>,
    #[validate(range(min = 1, max = 8760))]
    pub since_hours: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_request_valid() {
        let req = RegisterRequest {
            email: "user@example.com".into(),
            username: "johndoe".into(),
            password: "supersecret123".into(),
        };
        assert!(req.validate().is_ok());
    }

    #[test]
    fn test_register_request_invalid_email() {
        let req = RegisterRequest {
            email: "not-an-email".into(),
            username: "johndoe".into(),
            password: "supersecret123".into(),
        };
        assert!(req.validate().is_err());
    }

    #[test]
    fn test_register_request_short_username() {
        let req = RegisterRequest {
            email: "user@example.com".into(),
            username: "ab".into(),
            password: "supersecret123".into(),
        };
        assert!(req.validate().is_err());
    }

    #[test]
    fn test_register_request_short_password() {
        let req = RegisterRequest {
            email: "user@example.com".into(),
            username: "johndoe".into(),
            password: "short".into(),
        };
        assert!(req.validate().is_err());
    }

    #[test]
    fn test_login_request_valid_and_invalid() {
        let valid = LoginRequest {
            identifier: "admin".into(),
            password: "password123".into(),
        };
        assert!(valid.validate().is_ok());

        let empty_identifier = LoginRequest {
            identifier: "".into(),
            password: "password123".into(),
        };
        assert!(empty_identifier.validate().is_err());

        let short_password = LoginRequest {
            identifier: "admin".into(),
            password: "123".into(),
        };
        assert!(short_password.validate().is_err());
    }

    #[test]
    fn test_reset_password_request_validation() {
        let valid = ResetPasswordRequest {
            token: "reset-token-123".into(),
            password: "new-secure-password".into(),
        };
        assert!(valid.validate().is_ok());

        let empty_token = ResetPasswordRequest {
            token: "".into(),
            password: "new-secure-password".into(),
        };
        assert!(empty_token.validate().is_err());

        let short_pwd = ResetPasswordRequest {
            token: "reset-token-123".into(),
            password: "123".into(),
        };
        assert!(short_pwd.validate().is_err());
    }

    #[test]
    fn test_activity_log_query_validation() {
        let valid = ActivityLogQuery {
            offset: Some(10),
            limit: Some(25),
            activity_type: Some("LOGIN".into()),
        };
        assert!(valid.validate().is_ok());

        let limit_too_high = ActivityLogQuery {
            offset: Some(0),
            limit: Some(101),
            activity_type: None,
        };
        assert!(limit_too_high.validate().is_err());

        let offset_negative = ActivityLogQuery {
            offset: Some(-1),
            limit: Some(10),
            activity_type: None,
        };
        assert!(offset_negative.validate().is_err());
    }
}
