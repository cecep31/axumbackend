use super::tag::Tag;
use super::user::User;
use chrono::{DateTime, FixedOffset, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum OrderDirection {
    Asc,
    Desc,
}

#[derive(Serialize, Deserialize)]
pub struct SitemapPost {
    pub username: Option<String>,
    pub slug: String,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Serialize, Deserialize)]
pub struct Post {
    pub id: Uuid,
    pub title: String,
    pub body: Option<String>,
    #[serde(skip_serializing)]
    pub created_by: Uuid,
    pub slug: String,
    pub photo_url: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<DateTime<Utc>>,
    pub published: bool,
    pub published_at: Option<DateTime<Utc>>,
    pub view_count: i64,
    pub like_count: i64,
    pub bookmark_count: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<User>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<Tag>,
}

fn to_utc(value: Option<DateTime<FixedOffset>>) -> Option<DateTime<Utc>> {
    value.map(|dt| dt.with_timezone(&Utc))
}

impl Post {
    pub fn from_entity(
        post: crate::entities::posts::Model,
        user: Option<crate::entities::users::Model>,
        tags: Vec<crate::entities::tags::Model>,
        truncate_body: bool,
    ) -> Self {
        let body = post.body.map(|body| {
            if truncate_body && body.chars().count() > 250 {
                format!("{} ...", body.chars().take(250).collect::<String>())
            } else {
                body
            }
        });

        Self {
            id: post.id,
            title: post.title,
            body,
            created_by: post.created_by,
            slug: post.slug,
            photo_url: post.photo_url,
            created_at: to_utc(post.created_at),
            updated_at: to_utc(post.updated_at),
            deleted_at: to_utc(post.deleted_at),
            published: post.published.unwrap_or(true),
            published_at: to_utc(post.published_at),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_post_model(body: Option<String>) -> crate::entities::posts::Model {
        crate::entities::posts::Model {
            id: Uuid::now_v7(),
            created_at: None,
            updated_at: None,
            deleted_at: None,
            title: "Post Title".into(),
            created_by: Uuid::now_v7(),
            body,
            slug: "post-title".into(),
            photo_url: None,
            published: Some(true),
            published_at: None,
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
