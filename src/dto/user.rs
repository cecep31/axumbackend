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
    /// Raw so a missing / malformed id is a 422 (`required` / `uuid`) like
    /// echobackend's `validate:"required,uuid"`, not a body-parse 400.
    #[serde(default)]
    #[garde(custom(required_uuid))]
    pub user_id: String,
}

impl FollowRequest {
    /// The validated `user_id`.
    pub fn user_uuid(&self) -> Option<Uuid> {
        Uuid::parse_str(&self.user_id).ok()
    }
}

/// echobackend `required,uuid`: empty is `required`, anything but a
/// hyphenated UUID is `uuid`.
fn required_uuid(value: &str, _: &()) -> garde::Result {
    if value.is_empty() {
        return Err(garde::Error::new("required"));
    }
    if value.len() == 36 && Uuid::parse_str(value).is_ok() {
        Ok(())
    } else {
        Err(garde::Error::new("uuid"))
    }
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
    /// Raw, resolved leniently with `parse_pagination` (echobackend
    /// `ParsePaginationParams(10)`).
    #[garde(skip)]
    pub offset: Option<String>,
    #[garde(skip)]
    pub limit: Option<String>,
    #[garde(skip)]
    pub deleted: Option<String>,
    #[garde(skip)]
    pub order_by: Option<String>,
    #[garde(skip)]
    pub order_direction: Option<String>,
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
    #[garde(email, length(chars, max = 255))]
    pub email: String,
    #[garde(length(chars, min = 8, max = 128))]
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
    #[garde(email, length(chars, max = 255))]
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
    fn test_follow_request_requires_uuid() {
        let tag = |raw: &str| {
            FollowRequest {
                user_id: raw.into(),
            }
            .validate()
            .err()
            .map(|report| report.iter().next().unwrap().1.message().to_string())
        };
        assert_eq!(tag(""), Some("required".to_string()));
        assert_eq!(tag("not-a-uuid"), Some("uuid".to_string()));
        assert_eq!(tag("0190a8f2-1c3e-7b4a-9d6e-2f1a3b4c5d6e"), None);
    }

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
