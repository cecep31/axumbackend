use crate::models::post::Post;
use tokio_postgres::Client;
use uuid::Uuid;

pub async fn get_all_posts(client: &Client) -> Result<Vec<Post>, tokio_postgres::Error> {
    let rows = client.query("SELECT id, title, created_by, slug FROM posts ORDER BY id", &[]).await?;

    let posts: Result<Vec<Post>, _> = rows.iter().map(|row| {
        let id: Uuid = row.get(0);
        let title: String = row.get(1);
        let created_by: Uuid = row.get(2);
        let slug: String = row.get(3);

        Ok(Post {
            id,
            title,
            created_by,
            slug,
        })
    }).collect();

    posts
}
