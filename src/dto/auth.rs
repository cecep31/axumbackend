use garde::Validate;
use serde::Deserialize;

#[derive(Deserialize, Validate)]
pub struct RegisterRequest {
    #[garde(email, length(chars, max = 255))]
    pub email: String,
    #[garde(length(chars, min = 3, max = 30))]
    pub username: String,
    #[garde(length(chars, min = 8, max = 128))]
    pub password: String,
    #[garde(length(chars, min = 1, max = 100))]
    pub first_name: String,
    #[garde(length(chars, min = 1, max = 100))]
    pub last_name: String,
}

#[derive(Deserialize, Validate)]
pub struct LoginRequest {
    #[garde(length(chars, min = 1, max = 255))]
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
    #[garde(email, length(chars, max = 255))]
    pub email: String,
}

#[derive(Deserialize, Validate)]
pub struct ResetPasswordRequest {
    #[garde(length(chars, min = 1))]
    pub token: String,
    #[garde(length(chars, min = 8, max = 128))]
    pub password: String,
}

#[derive(Deserialize, Validate)]
pub struct ChangePasswordRequest {
    #[garde(length(chars, min = 8))]
    pub current_password: String,
    #[garde(length(chars, min = 8, max = 128))]
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
pub struct RevokeAllSessionsRequest {
    #[garde(skip)]
    pub confirm: Option<bool>,
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

/// `limit`/`offset` are raw strings resolved leniently with
/// `parse_pagination` (echobackend `ParsePaginationParams(20)`).
#[derive(Deserialize, Validate)]
pub struct ActivityLogQuery {
    #[garde(skip)]
    pub offset: Option<String>,
    #[garde(skip)]
    pub limit: Option<String>,
    #[garde(length(chars, min = 1, max = 50))]
    pub activity_type: Option<String>,
}

#[derive(Deserialize, Validate)]
pub struct RecentActivityQuery {
    #[garde(skip)]
    pub limit: Option<String>,
}

impl RecentActivityQuery {
    /// echobackend `GetRecentActivity`: only an integer in `1..=50` overrides
    /// the default of 10 (out-of-range values are ignored, not clamped).
    pub fn limit(&self) -> i64 {
        self.limit
            .as_deref()
            .and_then(|raw| raw.parse::<i64>().ok())
            .filter(|limit| (1..=50).contains(limit))
            .unwrap_or(10)
    }
}

/// `limit`/`offset` resolved like [`ActivityLogQuery`] (default 20).
#[derive(Deserialize, Validate)]
pub struct FailedLoginsQuery {
    #[garde(skip)]
    pub offset: Option<String>,
    #[garde(skip)]
    pub limit: Option<String>,
    /// Raw; resolved by [`FailedLoginsQuery::since_hours`].
    #[garde(skip)]
    pub since_hours: Option<String>,
}

impl FailedLoginsQuery {
    /// echobackend `GetFailedLogins`: a positive integer, otherwise 24 (never
    /// rejected; an oversized window is bounded by the service).
    pub fn since_hours(&self) -> i64 {
        self.since_hours
            .as_deref()
            .and_then(|raw| raw.parse::<i64>().ok())
            .filter(|hours| *hours > 0)
            .unwrap_or(24)
    }
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
            first_name: "John".into(),
            last_name: "Doe".into(),
        };
        assert!(req.validate().is_ok());
    }

    #[test]
    fn test_register_request_invalid_email() {
        let req = RegisterRequest {
            email: "not-an-email".into(),
            username: "johndoe".into(),
            password: "supersecret123".into(),
            first_name: "John".into(),
            last_name: "Doe".into(),
        };
        assert!(req.validate().is_err());
    }

    #[test]
    fn test_register_request_short_username() {
        let req = RegisterRequest {
            email: "user@example.com".into(),
            username: "ab".into(),
            password: "supersecret123".into(),
            first_name: "John".into(),
            last_name: "Doe".into(),
        };
        assert!(req.validate().is_err());
    }

    #[test]
    fn test_register_request_short_password() {
        let req = RegisterRequest {
            email: "user@example.com".into(),
            username: "johndoe".into(),
            password: "short".into(),
            first_name: "John".into(),
            last_name: "Doe".into(),
        };
        assert!(req.validate().is_err());
    }

    #[test]
    fn test_register_request_missing_names() {
        let req = RegisterRequest {
            email: "user@example.com".into(),
            username: "johndoe".into(),
            password: "supersecret123".into(),
            first_name: "".into(),
            last_name: "".into(),
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
    fn test_auth_request_max_lengths() {
        // echobackend dto/auth.go: password max=128, identifier/email max=255.
        let long_password = "a".repeat(129);
        let long_email = format!("{}@example.com", "a".repeat(250));
        let register = |email: &str, password: &str| RegisterRequest {
            email: email.into(),
            username: "testuser".into(),
            password: password.into(),
            first_name: "Test".into(),
            last_name: "User".into(),
        };
        assert!(
            register("a@example.com", &"a".repeat(128))
                .validate()
                .is_ok()
        );
        assert!(
            register("a@example.com", &long_password)
                .validate()
                .is_err()
        );
        assert!(register(&long_email, "password123").validate().is_err());

        let login = LoginRequest {
            identifier: "a".repeat(256),
            password: "password".into(),
        };
        assert!(login.validate().is_err());

        let forgot = ForgotPasswordRequest {
            email: long_email.clone(),
        };
        assert!(forgot.validate().is_err());

        let reset = ResetPasswordRequest {
            token: "pr_token".into(),
            password: long_password.clone(),
        };
        assert!(reset.validate().is_err());

        let change = ChangePasswordRequest {
            current_password: "a".repeat(200),
            new_password: long_password,
        };
        assert!(change.validate().is_err());
    }

    #[test]
    fn test_activity_log_query_pagination_is_lenient() {
        // echobackend never rejects pagination params: out-of-range or
        // non-numeric values fall back to the default / are clamped.
        let query = ActivityLogQuery {
            offset: Some("-1".into()),
            limit: Some("101".into()),
            activity_type: Some("LOGIN".into()),
        };
        assert!(query.validate().is_ok());
        assert_eq!(
            crate::dto::validation::parse_pagination(
                query.offset.as_deref(),
                query.limit.as_deref(),
                20
            ),
            (100, 0)
        );
    }

    #[test]
    fn test_recent_activity_limit() {
        let limit = |raw: Option<&str>| {
            RecentActivityQuery {
                limit: raw.map(Into::into),
            }
            .limit()
        };
        assert_eq!(limit(None), 10);
        assert_eq!(limit(Some("25")), 25);
        assert_eq!(limit(Some("51")), 10);
        assert_eq!(limit(Some("0")), 10);
        assert_eq!(limit(Some("abc")), 10);
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
