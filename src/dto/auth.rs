use serde::{Deserialize, Serialize};
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
pub struct CheckUsernameRequest {
    #[validate(length(min = 3, max = 30))]
    pub username: String,
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

#[derive(Deserialize, Validate)]
pub struct EmailPath {
    #[validate(email)]
    pub email: String,
}

#[derive(Serialize)]
pub struct AvailabilityResponse {
    pub username: Option<String>,
    pub email: Option<String>,
    pub available: bool,
}
