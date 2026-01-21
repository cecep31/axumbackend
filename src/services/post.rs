use crate::models::post::Post;
use tokio_postgres::Client;
use uuid::Uuid;

pub async fn get_all_posts(client: &Client) -> Result<Vec<Post>, tokio_postgres::Error> {
    let rows = client.query("SELECT id, title, body, created_by, slug FROM posts ORDER BY id", &[]).await?;

    let posts: Result<Vec<Post>, _> = rows.iter().map(|row| {
        let id: Uuid = row.get(0);
        let title: String = row.get(1);
        let body: String = row.get(2);
        let created_by: Uuid = row.get(3);
        let slug: String = row.get(4);

        Ok(Post {
            id,
            title,
            body,
            created_by,
            slug,
        })
    }).collect();

    posts
}

pub async fn get_random_posts(client: &Client, limit: i64) -> Result<Vec<Post>, tokio_postgres::Error> {
    let rows = client.query(
        "SELECT id, title, body, created_by, slug FROM posts ORDER BY RANDOM() LIMIT $1",
        &[&limit]
    ).await?;

    let posts: Result<Vec<Post>, _> = rows.iter().map(|row| {
        let id: Uuid = row.get(0);
        let title: String = row.get(1);
        let body: String = row.get(2);
        let created_by: Uuid = row.get(3);
        let slug: String = row.get(4);

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
        })
    }).collect();

    posts
}
