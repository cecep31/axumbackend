use super::*;

#[derive(Serialize, Deserialize)]
pub struct UserBrief {
    pub id: Uuid,
    pub email: String,
    pub username: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct AuthTokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub user: UserBrief,
}

#[derive(Serialize)]
pub struct RegisterResponse {
    pub id: Uuid,
    pub email: String,
    pub username: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
}

#[derive(Serialize)]
pub struct AuthActivityLogResponse {
    pub id: Uuid,
    pub user_id: Option<Uuid>,
    pub activity_type: String,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub status: String,
    pub error_message: Option<String>,
    /// The jsonb column as JSON text (echobackend scans it into `*string`),
    /// e.g. `"{\"provider\": \"github\"}"`, or null.
    pub metadata: Option<String>,
    pub created_at: chrono::DateTime<Utc>,
}

/// Flat profile subset returned by `GET /api/auth/profile`, mirroring
/// echobackend's `AuthHandler.GetProfile` map shape (no `name`, no nested
/// `profile`, no timestamps).
#[derive(Serialize)]
pub struct ProfileResponse {
    pub id: Uuid,
    pub email: String,
    pub username: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub image: Option<String>,
    pub is_super_admin: Option<bool>,
    pub followers_count: i64,
    pub following_count: i64,
}

#[derive(Debug)]
pub enum AuthError {
    Db(DbErr),
    UserExists,
    /// The authenticated user no longer exists or is soft-deleted
    /// (echobackend `ErrUserNotFound`).
    UserNotFound,
    InvalidCredentials,
    InvalidToken,
    TokenExpired,
    TokenUsed,
    Token(jsonwebtoken::errors::Error),
    Hash(String),
    Request(reqwest::Error),
    OAuth(String),
}

impl From<DbErr> for AuthError {
    fn from(err: DbErr) -> Self {
        Self::Db(err)
    }
}

impl From<reqwest::Error> for AuthError {
    fn from(err: reqwest::Error) -> Self {
        Self::Request(err)
    }
}

impl From<jsonwebtoken::errors::Error> for AuthError {
    fn from(err: jsonwebtoken::errors::Error) -> Self {
        Self::Token(err)
    }
}

impl From<bcrypt::BcryptError> for AuthError {
    fn from(err: bcrypt::BcryptError) -> Self {
        Self::Hash(err.to_string())
    }
}
