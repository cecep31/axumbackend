use crate::models::post::Post;
use crate::models::user::User;
use tokio_postgres::Client;
use uuid::Uuid;

pub async fn get_all_posts(client: &Client) -> Result<Vec<Post>, tokio_postgres::Error> {
    let rows = client.query(
        "SELECT p.id, p.title, p.body, p.created_by, p.slug, u.id, u.username 
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
        let user_id: Uuid = row.get(5);
        let username: String = row.get(6);

        Ok(Post {
            id,
            title,
            body,
            created_by,
            slug,
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
        "SELECT p.id, p.title, p.body, p.created_by, p.slug, u.id, u.username 
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
        let user_id: Uuid = row.get(5);
        let username: String = row.get(6);

        // Substring body to 200 characters max
        let body = if body.len() > 200 {
            format!("{}...", &body[..200])
        } else {
            body
        };

        Ok(Post {
            id,
            title,
            body,
            created_by,
            slug,
            creator: User {
                id: user_id,
                username,
            },
        })
    }).collect();

    posts
}
