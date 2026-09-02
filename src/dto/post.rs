use crate::dto::validation::{SLUG_RE, TAG_RE, USERNAME_RE};
use crate::services;
use serde::Deserialize;
use validator::Validate;

#[derive(Deserialize, Validate)]
pub struct CreatePostRequest {
    #[validate(length(min = 7))]
    pub title: String,
    pub photo_url: Option<String>,
    #[validate(length(min = 7))]
    pub slug: String,
    #[validate(length(min = 10))]
    pub body: String,
    #[serde(default)]
    pub published: bool,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Mirrors echobackend's `UpdatePostRequest` (`internal/dto/post.go`), which
/// carries no `validate` tags at all — empty-string fields are accepted and
/// simply ignored by the update service.
#[derive(Deserialize, Validate)]
pub struct UpdatePostRequest {
    pub title: Option<String>,
    pub photo_url: Option<String>,
    pub slug: Option<String>,
    pub body: Option<String>,
    pub published: Option<bool>,
    pub tags: Option<Vec<String>>,
}

#[derive(Deserialize, Validate)]
pub struct RandomPostQuery {
    #[validate(range(min = 1, max = 100))]
    pub limit: Option<i64>,
}

/// Analytics date filters. Kept as raw strings and parsed leniently, mirroring
/// echobackend's `MyPostsAnalyticsQuery` (invalid dates fall back to defaults).
#[derive(Deserialize, Validate)]
pub struct MyPostsAnalyticsQuery {
    pub start_date: Option<String>,
    pub end_date: Option<String>,
}

/// `months` is clamped leniently to `1..=24` (default `12`), mirroring
/// echobackend's `GetMyPostsLikesByMonth`.
#[derive(Deserialize, Validate)]
pub struct MyPostsLikesByMonthQuery {
    pub months: Option<i64>,
}

#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum OrderDirection {
    Asc,
    Desc,
}

impl From<OrderDirection> for services::post::SortDirection {
    fn from(value: OrderDirection) -> Self {
        match value {
            OrderDirection::Asc => Self::Asc,
            OrderDirection::Desc => Self::Desc,
        }
    }
}

#[derive(Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct PostPaginationQuery {
    #[validate(range(min = 0, max = 10_000))]
    pub offset: Option<i64>,
    #[validate(range(min = 1, max = 100))]
    pub limit: Option<i64>,
    #[validate(length(max = 200))]
    pub search: Option<String>,
    #[serde(alias = "sort_by")]
    pub order_by: Option<String>,
    #[serde(alias = "sort_order")]
    pub order_direction: Option<OrderDirection>,
}

/// Query filter for `GET /api/posts`, mirroring echobackend's `PostQueryFilter`
/// (`docs/api/posts.md`). Unlike most endpoints, the posts list accepts
/// `limit` values above 100. Values that echobackend parses leniently
/// (`published`, `created_by`, dates) are kept as raw strings here and
/// interpreted by the handler.
#[derive(Deserialize, Validate)]
pub struct PostsFilterQuery {
    #[validate(range(min = 0, max = 10_000))]
    pub offset: Option<i64>,
    #[validate(range(min = 1, max = 10_000))]
    pub limit: Option<i64>,
    #[validate(length(max = 200))]
    pub search: Option<String>,
    pub sort_by: Option<String>,
    pub sort_order: Option<String>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub created_by: Option<String>,
    pub published: Option<String>,
    pub tags: Option<String>,
}

#[derive(Deserialize, Validate)]
pub struct TagPath {
    #[validate(length(min = 1, max = 50), regex(path = *TAG_RE))]
    pub tag: String,
}

#[derive(Deserialize, Validate)]
pub struct PostPath {
    #[validate(length(min = 1, max = 50), regex(path = *USERNAME_RE))]
    pub username: String,
    #[validate(length(min = 1, max = 100), regex(path = *SLUG_RE))]
    pub slug: String,
}

pub fn post_pagination_params(
    query: &PostPaginationQuery,
) -> (
    i64,
    i64,
    Option<&str>,
    Option<&str>,
    Option<services::post::SortDirection>,
) {
    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(10);
    let search = query.search.as_deref();
    let order_by = query.order_by.as_deref();
    let order_direction = query.order_direction.map(Into::into);
    (offset, limit, search, order_by, order_direction)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_post_request_valid() {
        let req = CreatePostRequest {
            title: "A valid title".into(),
            photo_url: None,
            slug: "a-valid-slug".into(),
            body: "This is a valid body with more than 10 characters".into(),
            published: true,
            tags: vec!["rust".into()],
        };
        assert!(req.validate().is_ok());
    }

    #[test]
    fn test_create_post_request_validation_failures() {
        // Title too short (< 7)
        let req = CreatePostRequest {
            title: "Short".into(),
            photo_url: None,
            slug: "valid-slug".into(),
            body: "Valid body text here".into(),
            published: true,
            tags: vec![],
        };
        assert!(req.validate().is_err());

        // Slug too short (< 7)
        let req2 = CreatePostRequest {
            title: "Valid title".into(),
            photo_url: None,
            slug: "slug".into(),
            body: "Valid body text here".into(),
            published: true,
            tags: vec![],
        };
        assert!(req2.validate().is_err());

        // Body too short (< 10)
        let req3 = CreatePostRequest {
            title: "Valid title".into(),
            photo_url: None,
            slug: "valid-slug".into(),
            body: "Too short".into(),
            published: true,
            tags: vec![],
        };
        assert!(req3.validate().is_err());
    }

    #[test]
    fn test_tag_path_validation() {
        let valid = TagPath {
            tag: "rust-lang_2024".into(),
        };
        assert!(valid.validate().is_ok());

        let invalid_chars = TagPath {
            tag: "tag with space".into(),
        };
        assert!(invalid_chars.validate().is_err());

        let empty = TagPath { tag: "".into() };
        assert!(empty.validate().is_err());
    }

    #[test]
    fn test_post_path_validation() {
        let valid = PostPath {
            username: "john_doe-12".into(),
            slug: "my-first-post".into(),
        };
        assert!(valid.validate().is_ok());

        let invalid_username = PostPath {
            username: "invalid user".into(),
            slug: "my-first-post".into(),
        };
        assert!(invalid_username.validate().is_err());

        let invalid_slug = PostPath {
            username: "johndoe".into(),
            slug: "my_slug_underscores".into(), // SLUG_RE only allows [a-zA-Z0-9-]
        };
        assert!(invalid_slug.validate().is_err());
    }

    #[test]
    fn test_post_pagination_params_defaults() {
        let query = PostPaginationQuery {
            offset: None,
            limit: None,
            search: None,
            order_by: None,
            order_direction: None,
        };
        let (offset, limit, search, order_by, order_dir) = post_pagination_params(&query);
        assert_eq!(offset, 0);
        assert_eq!(limit, 10);
        assert_eq!(search, None);
        assert_eq!(order_by, None);
        assert!(order_dir.is_none());
    }

    #[test]
    fn test_post_pagination_params_custom() {
        let query = PostPaginationQuery {
            offset: Some(25),
            limit: Some(50),
            search: Some("rust".into()),
            order_by: Some("title".into()),
            order_direction: Some(OrderDirection::Asc),
        };
        let (offset, limit, search, order_by, order_dir) = post_pagination_params(&query);
        assert_eq!(offset, 25);
        assert_eq!(limit, 50);
        assert_eq!(search, Some("rust"));
        assert_eq!(order_by, Some("title"));
        assert!(matches!(
            order_dir,
            Some(services::post::SortDirection::Asc)
        ));
    }
}
