use super::tag::Tag;
use super::user::User;
use chrono::{DateTime, FixedOffset, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
pub struct SitemapPost {
    pub username: Option<String>,
    pub slug: String,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Post {
    pub id: Uuid,
    pub title: String,
    pub body: Option<String>,
    /// Internal only: never sent, and absent from the shared
    /// `posts:random` / `posts:trending` cache entries (echobackend's
    /// `PostResponse` has no such field), hence the default when decoding.
    #[serde(skip_serializing, default)]
    pub created_by: Uuid,
    pub slug: String,
    pub photo_url: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    /// `null` only for legacy rows with a NULL flag (echobackend `*bool`).
    pub published: Option<bool>,
    pub published_at: Option<DateTime<Utc>>,
    /// Always present (`null` unless hidden), like echobackend's `hidden_at`.
    pub hidden_at: Option<DateTime<Utc>>,
    pub view_count: i64,
    pub like_count: i64,
    pub bookmark_count: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<User>,
    /// echobackend `json:"tags,omitempty"`: may be absent in cached JSON.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<Tag>,
}

/// echobackend `dto.TruncatePostBodies(posts, 250)` limit, in chars (runes).
const LIST_BODY_MAX_CHARS: usize = 250;

/// The first [`LIST_BODY_MAX_CHARS`] chars plus `" ..."` when longer.
fn truncate_body(body: String) -> String {
    if body.chars().count() > LIST_BODY_MAX_CHARS {
        format!(
            "{} ...",
            body.chars().take(LIST_BODY_MAX_CHARS).collect::<String>()
        )
    } else {
        body
    }
}

fn to_utc(value: Option<DateTime<FixedOffset>>) -> Option<DateTime<Utc>> {
    value.map(|dt| dt.with_timezone(&Utc))
}

impl Post {
    /// echobackend `dto.TruncatePostBodies(posts, 250)`, applied after a
    /// cache read so cached entries keep the full body.
    pub fn truncate_bodies(posts: &mut [Post]) {
        for post in posts {
            post.body = post.body.take().map(truncate_body);
        }
    }

    pub fn from_entity(
        post: crate::entities::posts::Model,
        user: Option<crate::entities::users::Model>,
        tags: Vec<crate::entities::tags::Model>,
        truncate_body: bool,
    ) -> Self {
        let body = if truncate_body {
            post.body.map(self::truncate_body)
        } else {
            post.body
        };

        Self {
            id: post.id,
            title: post.title,
            body,
            created_by: post.created_by,
            slug: post.slug,
            photo_url: post.photo_url,
            created_at: to_utc(post.created_at),
            updated_at: to_utc(post.updated_at),
            published: post.published,
            published_at: to_utc(post.published_at),
            hidden_at: to_utc(post.hidden_at),
            view_count: post.view_count.unwrap_or_default(),
            like_count: post.like_count.unwrap_or_default(),
            bookmark_count: post.bookmark_count.unwrap_or_default(),
            user: user.map(Into::into),
            tags: tags.into_iter().map(Into::into).collect(),
        }
    }
}

impl SitemapPost {
    pub fn from_entities(
        post: crate::entities::posts::Model,
        user: crate::entities::users::Model,
    ) -> Self {
        Self {
            username: user.username,
            slug: post.slug,
            created_at: to_utc(post.created_at),
            updated_at: to_utc(post.updated_at),
        }
    }
}

/// `data` of `POST /api/posts`: `{ "id": <uuid> }`.
#[derive(Serialize)]
pub struct CreatedPostResponse {
    pub id: Uuid,
}

/// `data` of `POST /api/posts/image`: the stored object key, sent as both
/// `path` and `url` like echobackend.
#[derive(Serialize)]
pub struct PostImageUploadResponse {
    pub path: String,
    pub url: String,
}

impl PostImageUploadResponse {
    pub fn from_key(key: String) -> Self {
        Self {
            path: key.clone(),
            url: key,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn created_post_response_matches_old_json() {
        let id = Uuid::now_v7();
        assert_eq!(
            serde_json::to_value(CreatedPostResponse { id }).unwrap(),
            serde_json::json!({ "id": id })
        );
    }

    #[test]
    fn post_image_upload_response_matches_old_json() {
        let key = "posts/images/abc123.png".to_string();
        assert_eq!(
            serde_json::to_value(PostImageUploadResponse::from_key(key.clone())).unwrap(),
            serde_json::json!({ "path": key, "url": key })
        );
    }

    fn sample_post_model(body: Option<String>) -> crate::entities::posts::Model {
        crate::entities::posts::Model {
            id: Uuid::now_v7(),
            created_at: None,
            updated_at: None,
            title: "Post Title".into(),
            created_by: Uuid::now_v7(),
            body,
            slug: "post-title".into(),
            photo_url: None,
            published: Some(true),
            published_at: None,
            hidden_at: None,
            view_count: Some(5),
            like_count: Some(2),
            bookmark_count: Some(1),
        }
    }

    #[test]
    fn test_post_from_entity_truncates_long_body() {
        let long_body = "a".repeat(300);
        let model = sample_post_model(Some(long_body));
        let post = Post::from_entity(model, None, vec![], true);

        let body = post.body.unwrap();
        assert_eq!(body.len(), 254); // 250 chars + " ..."
        assert!(body.ends_with(" ..."));
    }

    #[test]
    fn test_post_from_entity_no_truncate_when_flag_false() {
        let long_body = "a".repeat(300);
        let model = sample_post_model(Some(long_body));
        let post = Post::from_entity(model, None, vec![], false);

        let body = post.body.unwrap();
        assert_eq!(body.len(), 300);
        assert!(!body.ends_with(" ..."));
    }

    #[test]
    fn test_post_from_entity_short_body_not_truncated() {
        let short_body = "Short content";
        let model = sample_post_model(Some(short_body.into()));
        let post = Post::from_entity(model, None, vec![], true);

        assert_eq!(post.body.unwrap(), "Short content");
    }

    #[test]
    fn test_post_json_always_has_hidden_at() {
        let model = sample_post_model(Some("body".into()));
        let json = serde_json::to_value(Post::from_entity(model, None, vec![], true)).unwrap();
        assert!(
            json.get("hidden_at")
                .is_some_and(serde_json::Value::is_null)
        );
        assert_eq!(json["published"], serde_json::json!(true));
        // `user`/`tags` stay omitempty, `created_by` is never sent.
        assert!(json.get("user").is_none());
        assert!(json.get("tags").is_none());
        assert!(json.get("created_by").is_none());
    }

    #[test]
    fn test_cached_post_json_without_created_by_decodes() {
        // echobackend's cached `PostResponse` (posts:random / posts:trending):
        // no `created_by`, `tags` omitted, full body.
        let body = "b".repeat(300);
        let cached = serde_json::json!([{
            "id": "0190a8f2-1c3e-7b4a-9d6e-2f1a3b4c5d6e",
            "title": "Post Title",
            "photo_url": null,
            "body": body,
            "slug": "post-title",
            "view_count": 5,
            "like_count": 2,
            "bookmark_count": 1,
            "published": true,
            "published_at": null,
            "hidden_at": null,
            "user": {"id": "0190a8f2-1c3e-7b4a-9d6e-2f1a3b4c5d6f", "username": "u", "image": null},
            "created_at": "2024-01-02T03:04:05Z",
            "updated_at": "2024-01-02T03:04:05Z"
        }]);
        let mut posts: Vec<Post> = serde_json::from_value(cached).unwrap();
        assert_eq!(posts[0].created_by, Uuid::nil());
        assert!(posts[0].tags.is_empty());
        assert_eq!(posts[0].body.as_deref().map(str::len), Some(300));

        // Round trip: what axum writes back decodes again.
        let written = serde_json::to_string(&posts).unwrap();
        assert!(serde_json::from_str::<Vec<Post>>(&written).is_ok());

        Post::truncate_bodies(&mut posts);
        let truncated = posts[0].body.as_deref().unwrap();
        assert_eq!(truncated.chars().count(), 254);
        assert!(truncated.ends_with(" ..."));
    }

    #[test]
    fn test_sitemap_post_from_entities() {
        let post_model = sample_post_model(None);
        let user_model = crate::entities::users::Model {
            id: Uuid::now_v7(),
            created_at: None,
            updated_at: None,
            deleted_at: None,
            first_name: None,
            last_name: None,
            email: "author@example.com".into(),
            password: None,
            image: None,
            is_super_admin: None,
            username: Some("blogauthor".into()),
            github_id: None,
            last_logged_at: None,
            followers_count: None,
            following_count: None,
        };

        let sitemap = SitemapPost::from_entities(post_model, user_model);
        assert_eq!(sitemap.username, Some("blogauthor".into()));
        assert_eq!(sitemap.slug, "post-title");
    }
}
