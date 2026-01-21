use crate::models::post::Post;
use crate::models::user::User;
use chrono::{DateTime, Utc};
use tokio_postgres::Client;
use uuid::Uuid;

pub async fn get_all_posts(client: &Client) -> Result<Vec<Post>, tokio_postgres::Error> {
    let rows = client.query(
        "SELECT p.id, p.title, p.body, p.created_by, p.slug, p.created_at, u.id, u.username 
         FROM posts p 
         INNER JOIN users u ON p.created_by = u.id 
         ORDER BY p.id",
        &[]
    ).await?;

    let posts: Result<Vec<Post>, _> = rows.iter().map(|row| {
        let id: Uuid = row.get(0);
        let title: String = row.get(1);
        let body: String = row.get(2);
        let created_by: Uuid = row.get(3);
        let slug: String = row.get(4);
        let created_at: DateTime<Utc> = row.get(5);
        let user_id: Uuid = row.get(6);
        let username: String = row.get(7);

        Ok(Post {
            id,
            title,
            body,
            created_by,
            slug,
            created_at,
            creator: User {
                id: user_id,
                username,
            },
        })
    }).collect();

    posts
}

pub async fn get_random_posts(client: &Client, limit: i64) -> Result<Vec<Post>, tokio_postgres::Error> {
    let rows = client.query(
        "SELECT p.id, p.title, p.body, p.created_by, p.slug, p.created_at, u.id, u.username 
         FROM posts p 
         INNER JOIN users u ON p.created_by = u.id 
         ORDER BY RANDOM() 
         LIMIT $1",
        &[&limit]
    ).await?;

    let posts: Result<Vec<Post>, _> = rows.iter().map(|row| {
        let id: Uuid = row.get(0);
        let title: String = row.get(1);
        let body: String = row.get(2);
        let created_by: Uuid = row.get(3);
        let slug: String = row.get(4);
        let created_at: DateTime<Utc> = row.get(5);
        let user_id: Uuid = row.get(6);
        let username: String = row.get(7);

        // Substring body to 200 characters max
        let body = if body.chars().count() > 200 {
            let truncated_body: String = body.chars().take(200).collect();
            format!("{}...", truncated_body)
        } else {
            body
        };

        Ok(Post {
            id,
            title,
            body,
            created_by,
            slug,
            created_at,
            creator: User {
                id: user_id,
                username,
            },
        })
    }).collect();

    posts
}
