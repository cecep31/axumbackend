use crate::dto::validation::parse_pagination;
use crate::services;
use garde::Validate;
use serde::Deserialize;

#[derive(Deserialize, Validate)]
pub struct CreatePostRequest {
    #[garde(length(chars, min = 7))]
    pub title: String,
    #[garde(skip)]
    pub photo_url: Option<String>,
    #[garde(length(chars, min = 7))]
    pub slug: String,
    #[garde(length(chars, min = 10))]
    pub body: String,
    #[serde(default)]
    #[garde(skip)]
    pub published: bool,
    #[serde(default)]
    #[garde(skip)]
    pub tags: Vec<String>,
}

/// Mirrors echobackend's `UpdatePostRequest` (`internal/dto/post.go`), which
/// carries no `validate` tags at all — empty-string fields are accepted and
/// simply ignored by the update service.
#[derive(Deserialize, Validate)]
pub struct UpdatePostRequest {
    #[garde(skip)]
    pub title: Option<String>,
    #[garde(skip)]
    pub photo_url: Option<String>,
    #[garde(skip)]
    pub slug: Option<String>,
    #[garde(skip)]
    pub body: Option<String>,
    #[garde(skip)]
    pub published: Option<bool>,
    #[garde(skip)]
    pub tags: Option<Vec<String>>,
}

/// `?limit=` for random/trending posts, parsed leniently like echobackend's
/// `ParsePaginationParams` (invalid values fall back to the default).
#[derive(Deserialize, Validate)]
pub struct RandomPostQuery {
    #[garde(skip)]
    pub limit: Option<String>,
}

impl RandomPostQuery {
    /// `limit` resolved with `default`, capped at 100.
    pub fn limit_or(&self, default: i64) -> i64 {
        parse_pagination(None, self.limit.as_deref(), default).0
    }
}

/// echobackend `maxSitemapPostLimit`: default and cap of the sitemap list.
pub const MAX_SITEMAP_POST_LIMIT: i64 = 50_000;

/// `GET /api/posts/sitemap?limit=`: a positive integer is used (capped at
/// [`MAX_SITEMAP_POST_LIMIT`]); anything else means the default (50000).
#[derive(Deserialize, Validate)]
pub struct SitemapQuery {
    #[garde(skip)]
    pub limit: Option<String>,
}

impl SitemapQuery {
    pub fn limit(&self) -> i64 {
        self.limit
            .as_deref()
            .and_then(|raw| raw.parse::<i64>().ok())
            .filter(|limit| *limit > 0)
            .unwrap_or(MAX_SITEMAP_POST_LIMIT)
            .min(MAX_SITEMAP_POST_LIMIT)
    }
}

/// Analytics date filters. Kept as raw strings and parsed leniently, mirroring
/// echobackend's `MyPostsAnalyticsQuery` (invalid dates fall back to defaults).
#[derive(Deserialize, Validate)]
pub struct MyPostsAnalyticsQuery {
    #[garde(skip)]
    pub start_date: Option<String>,
    #[garde(skip)]
    pub end_date: Option<String>,
}

/// `months` is clamped leniently to `1..=24` (default `12`), mirroring
/// echobackend's `GetMyPostsLikesByMonth`.
#[derive(Deserialize, Validate)]
pub struct MyPostsLikesByMonthQuery {
    #[garde(skip)]
    pub months: Option<i64>,
}

/// echobackend `PostQueryFilter.GetSortOrder`: exactly `asc` or `desc`,
/// anything else falls back to the default (desc), represented by `None`.
pub fn sort_direction(raw: Option<&str>) -> Option<services::post::SortDirection> {
    match raw {
        Some("asc") => Some(services::post::SortDirection::Asc),
        Some("desc") => Some(services::post::SortDirection::Desc),
        _ => None,
    }
}

/// Go `strconv.ParseBool`; an unparseable value is `None` (ignored, never a
/// 400), like echobackend's `published` query handling.
pub fn parse_go_bool(raw: Option<&str>) -> Option<bool> {
    raw.and_then(crate::config::go_parse_bool)
}

/// Lenient query for the post lists (`/me`, `/feed/for-you`, `/username/..`,
/// `/tag/..`, `/{id}/views`, `/{id}/likes`): nothing here is ever rejected.
/// `sort_by`/`orderBy` and `sort_order`/`orderDirection` are separate keys so
/// sending both never fails deserialization; the snake_case one wins, like
/// echobackend's `GetPostsByTag`.
#[derive(Deserialize, Validate)]
pub struct PostPaginationQuery {
    /// Raw, resolved leniently by [`PostPaginationQuery::resolve`].
    #[garde(skip)]
    pub offset: Option<String>,
    #[garde(skip)]
    pub limit: Option<String>,
    /// `GET /api/posts/me` only: `title ILIKE '%search%'` when non-empty.
    #[garde(skip)]
    pub search: Option<String>,
    /// `GET /api/posts/me` only: Go `strconv.ParseBool`, ignored when invalid.
    #[garde(skip)]
    pub published: Option<String>,
    #[garde(skip)]
    pub sort_by: Option<String>,
    #[serde(rename = "orderBy")]
    #[garde(skip)]
    pub order_by: Option<String>,
    #[garde(skip)]
    pub sort_order: Option<String>,
    #[serde(rename = "orderDirection")]
    #[garde(skip)]
    pub order_direction: Option<String>,
}

/// Query filter for `GET /api/posts`, mirroring echobackend's `PostQueryFilter`
/// (`internal/handler/post_handler.go` `GetPosts`): `limit` defaults to 10 and
/// is capped at 100, there is no `published` filter (the list is always
/// published posts only) and `search` has no length cap. `created_by` and the
/// dates are kept as raw strings and interpreted by the handler.
#[derive(Deserialize, Validate)]
pub struct PostsFilterQuery {
    /// Raw, resolved leniently by [`PostsFilterQuery::resolve`].
    #[garde(skip)]
    pub offset: Option<String>,
    #[garde(skip)]
    pub limit: Option<String>,
    #[garde(skip)]
    pub search: Option<String>,
    #[garde(skip)]
    pub sort_by: Option<String>,
    #[garde(skip)]
    pub sort_order: Option<String>,
    #[garde(skip)]
    pub start_date: Option<String>,
    #[garde(skip)]
    pub end_date: Option<String>,
    #[garde(skip)]
    pub created_by: Option<String>,
    #[garde(skip)]
    pub tags: Option<String>,
}

impl PostsFilterQuery {
    /// `(limit, offset)` like echobackend's `GetPosts`: default 10, capped at
    /// 100 (`maxPostLimit`), offset used only when > 0.
    pub fn resolve(&self) -> (i64, i64) {
        parse_pagination(self.offset.as_deref(), self.limit.as_deref(), 10)
    }
}

impl PostPaginationQuery {
    /// `(limit, offset)` via `ParsePaginationParams(default_limit)`.
    pub fn resolve(&self, default_limit: i64) -> (i64, i64) {
        parse_pagination(self.offset.as_deref(), self.limit.as_deref(), default_limit)
    }
}

/// `/api/posts/tag/{tag}`: taken raw like echobackend's `c.Param("tag")`; an
/// unknown or odd tag simply yields an empty page.
#[derive(Deserialize, Validate)]
pub struct TagPath {
    #[garde(skip)]
    pub tag: String,
}

/// `/api/posts/username/{username}`: taken raw like echobackend's
/// `c.Param("username")`; an unknown or odd username yields an empty page.
#[derive(Deserialize, Validate)]
pub struct PostsUsernamePath {
    #[garde(skip)]
    pub username: String,
}

#[derive(Deserialize, Validate)]
pub struct PostPath {
    // Any non-empty username (echobackend `c.Param`): no charset rule.
    #[garde(length(chars, min = 1))]
    pub username: String,
    // No charset rule: post creation only enforces a minimum length (same as
    // echobackend), so stored slugs may hold any character and must stay
    // reachable. Max mirrors the varchar(255) column.
    #[garde(length(chars, min = 1, max = 255))]
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
    let (limit, offset) = query.resolve(10);
    let search = query.search.as_deref();
    fn non_empty(value: &Option<String>) -> Option<&str> {
        value.as_deref().filter(|v| !v.is_empty())
    }
    let order_by = non_empty(&query.sort_by).or(non_empty(&query.order_by));
    let order_direction =
        sort_direction(non_empty(&query.sort_order).or(non_empty(&query.order_direction)));
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
    fn test_posts_path_params_are_not_validated() {
        // echobackend takes these raw: an odd value is a normal (empty) lookup.
        assert!(
            TagPath {
                tag: "tag with space".into()
            }
            .validate()
            .is_ok()
        );
        assert!(TagPath { tag: "".into() }.validate().is_ok());
        assert!(
            PostsUsernamePath {
                username: "x".repeat(80)
            }
            .validate()
            .is_ok()
        );
    }

    #[test]
    fn test_sitemap_limit_like_echobackend() {
        let limit = |raw: Option<&str>| {
            SitemapQuery {
                limit: raw.map(Into::into),
            }
            .limit()
        };
        assert_eq!(limit(None), 50_000);
        assert_eq!(limit(Some("250")), 250);
        assert_eq!(limit(Some("100000")), 50_000);
        assert_eq!(limit(Some("-5")), 50_000);
        assert_eq!(limit(Some("0")), 50_000);
        assert_eq!(limit(Some("abc")), 50_000);
    }

    #[test]
    fn test_parse_go_bool_is_lenient() {
        for raw in ["1", "t", "T", "TRUE", "true", "True"] {
            assert_eq!(parse_go_bool(Some(raw)), Some(true), "{raw}");
        }
        for raw in ["0", "f", "F", "FALSE", "false", "False"] {
            assert_eq!(parse_go_bool(Some(raw)), Some(false), "{raw}");
        }
        assert_eq!(parse_go_bool(Some("yes")), None);
        assert_eq!(parse_go_bool(Some("")), None);
        assert_eq!(parse_go_bool(None), None);
    }

    #[test]
    fn test_post_path_validation() {
        let valid = PostPath {
            username: "john_doe-12".into(),
            slug: "my-first-post".into(),
        };
        assert!(valid.validate().is_ok());

        // Any non-empty username is accepted (echobackend has no charset rule).
        let dotted_username = PostPath {
            username: "john.doe".into(),
            slug: "my-first-post".into(),
        };
        assert!(dotted_username.validate().is_ok());

        let empty_username = PostPath {
            username: "".into(),
            slug: "my-first-post".into(),
        };
        assert!(empty_username.validate().is_err());

        let legacy_slug = PostPath {
            username: "johndoe".into(),
            slug: "my_slug_underscores".into(),
        };
        assert!(legacy_slug.validate().is_ok());

        let empty_slug = PostPath {
            username: "johndoe".into(),
            slug: "".into(),
        };
        assert!(empty_slug.validate().is_err());
    }

    #[test]
    fn test_post_pagination_params_defaults() {
        let query = PostPaginationQuery {
            offset: None,
            limit: None,
            search: None,
            published: None,
            sort_by: None,
            order_by: None,
            sort_order: None,
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
            offset: Some("25".into()),
            limit: Some("50".into()),
            search: Some("rust".into()),
            published: None,
            sort_by: None,
            order_by: Some("title".into()),
            sort_order: None,
            order_direction: Some("asc".into()),
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

    #[test]
    fn test_post_pagination_is_lenient_like_echobackend() {
        let query = PostPaginationQuery {
            offset: Some("20000".into()),
            limit: Some("500".into()),
            search: None,
            published: None,
            sort_by: None,
            order_by: None,
            sort_order: None,
            order_direction: None,
        };
        assert_eq!(query.resolve(10), (100, 20000));

        let query = PostPaginationQuery {
            offset: Some("-1".into()),
            limit: Some("abc".into()),
            search: None,
            published: None,
            sort_by: None,
            order_by: None,
            sort_order: None,
            order_direction: None,
        };
        assert_eq!(query.resolve(10), (10, 0));
        assert!(query.validate().is_ok());

        let both = PostPaginationQuery {
            offset: None,
            limit: None,
            search: None,
            published: None,
            sort_by: Some("view_count".into()),
            order_by: Some("title".into()),
            sort_order: Some("bogus".into()),
            order_direction: Some("asc".into()),
        };
        let (_, _, _, order_by, order_dir) = post_pagination_params(&both);
        assert_eq!(order_by, Some("view_count"));
        // An invalid sort_order wins over orderDirection and means desc.
        assert!(order_dir.is_none());

        let random = RandomPostQuery {
            limit: Some("0".into()),
        };
        assert_eq!(random.limit_or(9), 9);
    }
}
