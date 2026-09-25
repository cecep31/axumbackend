use garde::Validate;
use serde::Deserialize;

#[derive(Deserialize, Validate)]
pub struct RegisterRequest {
    #[garde(email)]
    pub email: String,
    #[garde(length(chars, min = 3, max = 30))]
    pub username: String,
    #[garde(length(chars, min = 8))]
    pub password: String,
}

#[derive(Deserialize, Validate)]
pub struct LoginRequest {
    #[garde(length(chars, min = 1))]
    pub identifier: String,
    #[garde(length(chars, min = 6))]
    pub password: String,
}

#[derive(Deserialize, Validate)]
pub struct RefreshTokenRequest {
    #[garde(length(chars, min = 1))]
    pub refresh_token: String,
}

#[derive(Deserialize, Validate)]
pub struct ForgotPasswordRequest {
    #[garde(email)]
    pub email: String,
}

#[derive(Deserialize, Validate)]
pub struct ResetPasswordRequest {
    #[garde(length(chars, min = 1))]
    pub token: String,
    #[garde(length(chars, min = 8))]
    pub password: String,
}

#[derive(Deserialize, Validate)]
pub struct ChangePasswordRequest {
    #[garde(length(chars, min = 8))]
    pub current_password: String,
    #[garde(length(chars, min = 8))]
    pub new_password: String,
}

#[derive(Deserialize, Validate)]
pub struct LogoutRequest {
    #[garde(length(chars, min = 1))]
    pub refresh_token: String,
}

#[derive(Deserialize, Validate)]
pub struct OAuthExchangeRequest {
    #[garde(length(chars, min = 1))]
    pub code: String,
}

#[derive(Deserialize, Validate)]
pub struct CheckUsernameRequest {
    #[garde(length(chars, min = 3, max = 30))]
    pub username: String,
}

#[derive(Deserialize, Validate)]
pub struct UpdateProfileRequest {
    #[garde(length(chars, min = 3, max = 30))]
    pub username: String,
    #[garde(length(chars, max = 100))]
    pub first_name: Option<String>,
    #[garde(length(chars, max = 100))]
    pub last_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GithubCallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
}

#[derive(Deserialize, Validate)]
pub struct ActivityLogQuery {
    #[garde(range(min = 0, max = 10_000))]
    pub offset: Option<i64>,
    #[garde(range(min = 1, max = 100))]
    pub limit: Option<i64>,
    #[garde(length(chars, min = 1, max = 50))]
    pub activity_type: Option<String>,
}

#[derive(Deserialize, Validate)]
pub struct RecentActivityQuery {
    #[garde(range(min = 1, max = 50))]
    pub limit: Option<i64>,
}

#[derive(Deserialize, Validate)]
pub struct FailedLoginsQuery {
    #[garde(range(min = 0, max = 10_000))]
    pub offset: Option<i64>,
    #[garde(range(min = 1, max = 100))]
    pub limit: Option<i64>,
    #[garde(range(min = 1, max = 8760))]
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

    #[test]
    fn test_check_username_request_validation() {
        let valid = CheckUsernameRequest {
            username: "johndoe".into(),
        };
        assert!(valid.validate().is_ok());

        let short = CheckUsernameRequest {
            username: "ab".into(),
        };
        assert!(short.validate().is_err());

        let long = CheckUsernameRequest {
            username: "a".repeat(31),
        };
        assert!(long.validate().is_err());
    }

    #[test]
    fn test_update_profile_request_validation() {
        let valid = UpdateProfileRequest {
            username: "johndoe".into(),
            first_name: Some("John".into()),
            last_name: Some("Doe".into()),
        };
        assert!(valid.validate().is_ok());

        let short_username = UpdateProfileRequest {
            username: "jo".into(),
            first_name: None,
            last_name: None,
        };
        assert!(short_username.validate().is_err());

        let long_name = UpdateProfileRequest {
            username: "johndoe".into(),
            first_name: Some("a".repeat(101)),
            last_name: None,
        };
        assert!(long_name.validate().is_err());
    }
}
