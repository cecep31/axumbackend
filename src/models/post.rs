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
    pub deleted_at: Option<DateTime<Utc>>,
    pub published: bool,
    pub published_at: Option<DateTime<Utc>>,
    pub view_count: i64,
    pub like_count: i64,
    pub bookmark_count: i64,
    pub user: Option<User>,
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
