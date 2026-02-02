use crate::models::tag::Tag;
use tokio_postgres::Client;

pub async fn get_all_tags(
    client: &Client,
    offset: i64,
    limit: i64,
) -> Result<(Vec<Tag>, i64), tokio_postgres::Error> {
    // Get total count
    let total: i64 = client
        .query_one("SELECT COUNT(*) FROM tags", &[])
        .await?
        .get(0);

    // Get paginated tags
    let rows = client
        .query(
            "SELECT id, name, created_at FROM tags ORDER BY name LIMIT $1 OFFSET $2",
            &[&limit, &offset],
        )
        .await?;

    let tags: Vec<Tag> = rows.iter().map(Tag::from).collect();

    Ok((tags, total))
}
