use serde::Deserialize;
use uuid::Uuid;
use validator::Validate;

#[derive(Deserialize, Validate)]
pub struct UserIdPath {
    pub id: Uuid,
}

#[derive(Deserialize, Validate)]
pub struct FollowRequest {
    pub user_id: Uuid,
}

/// Mirrors echobackend's `UserDeletedFilter` (`internal/dto/user.go`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum UserDeletedFilter {
    #[default]
    Active,
    Only,
    All,
}

impl UserDeletedFilter {
    pub fn parse(value: Option<&str>) -> Result<Self, String> {
        match value {
            None | Some("") | Some("false") => Ok(Self::Active),
            Some("true") => Ok(Self::Only),
            Some("all") => Ok(Self::All),
            _ => Err("deleted must be true, false, or all".to_string()),
        }
    }
}

#[derive(Deserialize, Validate)]
pub struct UserListQuery {
    #[validate(range(min = 0, max = 10_000))]
    pub offset: Option<i64>,
    #[validate(range(min = 1, max = 100))]
    pub limit: Option<i64>,
    pub deleted: Option<String>,
}

#[derive(Deserialize, Validate)]
pub struct UserDetailQuery {
    pub deleted: Option<String>,
}
