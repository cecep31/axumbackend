use garde::Validate;
use serde::Deserialize;
use uuid::Uuid;

#[derive(Deserialize, Validate)]
pub struct UserIdPath {
    #[garde(skip)]
    pub id: Uuid,
}

#[derive(Deserialize, Validate)]
pub struct FollowRequest {
    #[garde(skip)]
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
    #[garde(range(min = 0, max = 10_000))]
    pub offset: Option<i64>,
    #[garde(range(min = 1, max = 100))]
    pub limit: Option<i64>,
    #[garde(skip)]
    pub deleted: Option<String>,
}

#[derive(Deserialize, Validate)]
pub struct UserDetailQuery {
    #[garde(skip)]
    pub deleted: Option<String>,
}

#[derive(Deserialize, Validate)]
pub struct CreateUserRequest {
    #[garde(length(chars, min = 3, max = 30))]
    pub username: String,
    #[garde(email)]
    pub email: String,
    #[garde(length(chars, min = 8))]
    pub password: String,
    #[garde(length(chars, max = 100))]
    pub first_name: Option<String>,
    #[garde(length(chars, max = 100))]
    pub last_name: Option<String>,
}

#[derive(Deserialize, Validate)]
pub struct UpdateUserRequest {
    #[garde(length(chars, min = 3, max = 30))]
    pub username: String,
    #[garde(email)]
    pub email: String,
    #[garde(length(chars, max = 100))]
    pub first_name: Option<String>,
    #[garde(length(chars, max = 100))]
    pub last_name: Option<String>,
    #[serde(default)]
    #[garde(skip)]
    pub is_super_admin: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_deleted_filter_parse() {
        assert_eq!(
            UserDeletedFilter::parse(None),
            Ok(UserDeletedFilter::Active)
        );
        assert_eq!(
            UserDeletedFilter::parse(Some("")),
            Ok(UserDeletedFilter::Active)
        );
        assert_eq!(
            UserDeletedFilter::parse(Some("false")),
            Ok(UserDeletedFilter::Active)
        );
        assert_eq!(
            UserDeletedFilter::parse(Some("true")),
            Ok(UserDeletedFilter::Only)
        );
        assert_eq!(
            UserDeletedFilter::parse(Some("all")),
            Ok(UserDeletedFilter::All)
        );
        assert!(UserDeletedFilter::parse(Some("invalid")).is_err());
    }

    #[test]
    fn test_create_user_request_validation() {
        let valid = CreateUserRequest {
            username: "newuser".into(),
            email: "user@example.com".into(),
            password: "password123".into(),
            first_name: Some("New".into()),
            last_name: Some("User".into()),
        };
        assert!(valid.validate().is_ok());

        let invalid_email = CreateUserRequest {
            username: "newuser".into(),
            email: "not-an-email".into(),
            password: "password123".into(),
            first_name: None,
            last_name: None,
        };
        assert!(invalid_email.validate().is_err());

        let short_pwd = CreateUserRequest {
            username: "newuser".into(),
            email: "user@example.com".into(),
            password: "short".into(),
            first_name: None,
            last_name: None,
        };
        assert!(short_pwd.validate().is_err());
    }

    #[test]
    fn test_update_user_request_validation() {
        let valid = UpdateUserRequest {
            username: "updateduser".into(),
            email: "updated@example.com".into(),
            first_name: Some("Updated".into()),
            last_name: None,
            is_super_admin: Some(true),
        };
        assert!(valid.validate().is_ok());

        let short_username = UpdateUserRequest {
            username: "u".into(),
            email: "updated@example.com".into(),
            first_name: None,
            last_name: None,
            is_super_admin: None,
        };
        assert!(short_username.validate().is_err());
    }
}
