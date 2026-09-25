use serde::Deserialize;
use uuid::Uuid;
use validator::{Validate, ValidationError};

/// Mirrors echobackend's `omitempty,uuid` tag on `reply_to_id`: the field is
/// kept as a string so a malformed id fails validation (`422`) instead of body
/// parsing (`400`).
fn validate_uuid(value: &str) -> Result<(), ValidationError> {
    Uuid::parse_str(value)
        .map(|_| ())
        .map_err(|_| ValidationError::new("uuid"))
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateGuildRequest {
    #[validate(length(min = 3, max = 100))]
    pub name: String,
    #[validate(length(min = 3, max = 100))]
    pub slug: Option<String>,
    #[validate(length(max = 2000))]
    pub description: Option<String>,
    #[validate(url, length(max = 2048))]
    pub avatar_url: Option<String>,
    pub is_public: Option<bool>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpdateGuildRequest {
    #[validate(length(min = 3, max = 100))]
    pub name: Option<String>,
    #[validate(length(max = 2000))]
    pub description: Option<String>,
    #[validate(url, length(max = 2048))]
    pub avatar_url: Option<String>,
    pub is_public: Option<bool>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateGuildChannelRequest {
    #[validate(length(min = 1, max = 100))]
    pub name: String,
    #[validate(length(max = 1024))]
    pub topic: Option<String>,
    #[validate(range(min = 0))]
    pub position: Option<i32>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpdateGuildChannelRequest {
    #[validate(length(min = 1, max = 100))]
    pub name: Option<String>,
    #[validate(length(max = 1024))]
    pub topic: Option<String>,
    #[validate(range(min = 0))]
    pub position: Option<i32>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateGuildMessageRequest {
    #[validate(length(min = 1, max = 4000))]
    pub content: String,
    #[validate(custom(function = "validate_uuid"))]
    pub reply_to_id: Option<String>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpdateGuildMessageRequest {
    #[validate(length(min = 1, max = 4000))]
    pub content: String,
}

/// Guild directory query, mirroring echobackend's
/// `ParsePaginationParams(c, 20)` plus `?search=`.
#[derive(Deserialize, Validate)]
pub struct GuildListQuery {
    pub limit: Option<String>,
    pub offset: Option<String>,
    pub search: Option<String>,
}

impl GuildListQuery {
    pub fn resolve(&self) -> (i64, i64) {
        crate::dto::validation::parse_pagination(self.offset.as_deref(), self.limit.as_deref(), 20)
    }
}

/// Channel history query: `?limit=` (default 50, max 100) and the
/// `?before=<message id>` cursor.
#[derive(Deserialize, Validate)]
pub struct GuildMessagesQuery {
    pub limit: Option<String>,
    pub before: Option<String>,
}

impl GuildMessagesQuery {
    pub fn resolve_limit(&self) -> i64 {
        crate::dto::validation::parse_pagination(None, self.limit.as_deref(), 50).0
    }
}

// Ids in guild paths stay strings: echobackend looks a malformed id up as
// "not found" after the guild checks, so a hidden guild keeps reading as
// missing instead of failing path parsing with a 400.

#[derive(Deserialize, Validate)]
pub struct GuildSlugPath {
    pub slug: String,
}

#[derive(Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct GuildChannelPath {
    pub slug: String,
    pub channel_id: String,
}

#[derive(Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct GuildMessagePath {
    pub slug: String,
    pub channel_id: String,
    pub message_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guild_request(name: &str, avatar_url: Option<&str>) -> CreateGuildRequest {
        CreateGuildRequest {
            name: name.into(),
            slug: None,
            description: None,
            avatar_url: avatar_url.map(Into::into),
            is_public: None,
        }
    }

    #[test]
    fn test_create_guild_request_validation() {
        assert!(
            guild_request("Rustaceans", Some("https://example.com/a.png"))
                .validate()
                .is_ok()
        );
        assert!(guild_request("ab", None).validate().is_err());
        assert!(
            guild_request("Rustaceans", Some("not a url"))
                .validate()
                .is_err()
        );
    }

    #[test]
    fn test_create_message_request_validation() {
        let ok = CreateGuildMessageRequest {
            content: "hi".into(),
            reply_to_id: Some(Uuid::now_v7().to_string()),
        };
        assert!(ok.validate().is_ok());

        let bad_reply = CreateGuildMessageRequest {
            content: "hi".into(),
            reply_to_id: Some("nope".into()),
        };
        assert!(bad_reply.validate().is_err());

        let too_long = CreateGuildMessageRequest {
            content: "x".repeat(4001),
            reply_to_id: None,
        };
        assert!(too_long.validate().is_err());
    }
}
