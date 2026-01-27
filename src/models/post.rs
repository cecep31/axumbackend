use super::tag::Tag;
use super::user::User;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio_postgres::Row;
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
pub struct Post {
    pub id: Uuid,
    pub title: String,
    pub body: Option<String>,
    pub created_by: Uuid,
    pub slug: String,
    pub photo_url: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub published: bool,
    pub view_count: i64,
    pub like_count: i64,
    pub user: User,
    pub tags: Vec<Tag>,
}

impl From<&Row> for Post {
    fn from(row: &Row) -> Self {
        // Substring body to 200 characters max
        let body: Option<String> = row.get(2);
        let body = body.map(|b| {
            if b.chars().count() > 200 {
                let truncated_body: String = b.chars().take(200).collect();
                format!("{}...", truncated_body)
            } else {
                b
            }
        });

        Self {
            id: row.get(0),
            title: row.get(1),
            body,
            created_by: row.get(3),
            slug: row.get(4),
            photo_url: row.get(5),
            created_at: row.get(6),
            updated_at: row.get(7),
            deleted_at: row.get(8),
            published: row.get(9),
            view_count: row.get(10),
            like_count: row.get(11),
            user: User {
                id: row.get(12),
                username: row.get(13),
            },
            tags: Vec::new(),
        }
    }
}

impl Post {
    /// Create a Post from a Row without truncating the body
    pub fn from_full(row: &Row) -> Self {
        let body: Option<String> = row.get(2);

        Self {
            id: row.get(0),
            title: row.get(1),
            body,
            created_by: row.get(3),
            slug: row.get(4),
            photo_url: row.get(5),
            created_at: row.get(6),
            updated_at: row.get(7),
            deleted_at: row.get(8),
            published: row.get(9),
            view_count: row.get(10),
            like_count: row.get(11),
            user: User {
                id: row.get(12),
                username: row.get(13),
            },
            tags: Vec::new(),
        }
    }
}
