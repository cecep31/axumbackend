use crate::models::post::Post;
use crate::models::tag::Tag;
use std::collections::HashMap;
use tokio_postgres::Client;

/// Escape special LIKE/ILIKE pattern characters (% and _) to prevent pattern injection
fn escape_like_pattern(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// Fetch and assign tags to posts in a single batch query (avoids N+1)
async fn fetch_tags_for_posts(
    client: &Client,
    posts: &mut [Post],
) -> Result<(), tokio_postgres::Error> {
    if posts.is_empty() {
        return Ok(());
    }

    let post_ids: Vec<uuid::Uuid> = posts.iter().map(|p| p.id).collect();
    let rows = client
        .query(
            "SELECT t.id, t.name, t.created_at, ptt.post_id
             FROM tags t
             INNER JOIN posts_to_tags ptt ON t.id = ptt.tag_id
             WHERE ptt.post_id = ANY($1)
             ORDER BY t.name",
            &[&post_ids],
        )
        .await?;

    let mut tags_by_post: HashMap<uuid::Uuid, Vec<Tag>> = HashMap::new();
    for row in &rows {
        let post_id: uuid::Uuid = row.get(3);
        tags_by_post
            .entry(post_id)
            .or_default()
            .push(Tag::from(row));
    }

    for post in posts {
        post.tags = tags_by_post.remove(&post.id).unwrap_or_default();
    }

    Ok(())
}

/// Validate order_by field against whitelist
fn validate_order_field(order_by: Option<&str>) -> &str {
    const VALID_FIELDS: [&str; 6] = [
        "id",
        "title",
        "created_at",
        "updated_at",
        "view_count",
        "like_count",
    ];
    order_by
        .filter(|f| VALID_FIELDS.contains(f))
        .unwrap_or("created_at")
}

/// Get order direction string
fn get_order_dir(dir: Option<&crate::handlers::OrderDirection>) -> &'static str {
    match dir {
        Some(crate::handlers::OrderDirection::Asc) => "ASC",
        _ => "DESC", // Default to DESC
    }
}

pub async fn get_all_posts(
    client: &Client,
    offset: i64,
    limit: i64,
    search: Option<&str>,
    order_by: Option<&str>,
    order_direction: Option<&crate::handlers::OrderDirection>,
) -> Result<(Vec<Post>, i64), tokio_postgres::Error> {
    let order_field = validate_order_field(order_by);
    let order_dir = get_order_dir(order_direction);
    let search_param = search.map(|s| format!("%{}%", escape_like_pattern(s)));

    // Get total count
    let total: i64 = if let Some(ref search_val) = search_param {
        client
            .query_one(
                "SELECT COUNT(*) FROM posts p INNER JOIN users u ON p.created_by = u.id 
                 WHERE p.published = true AND (p.title ILIKE $1 OR p.body ILIKE $1 OR u.username ILIKE $1)",
                &[search_val],
            )
            .await?
            .get(0)
    } else {
        client
            .query_one("SELECT COUNT(*) FROM posts WHERE published = true", &[])
            .await?
            .get(0)
    };

    // Build and execute main query
    let query = if search_param.is_some() {
        format!(
            "SELECT p.id, p.title, p.body, p.created_by, p.slug, p.photo_url, p.created_at, p.updated_at, p.deleted_at, p.published, p.view_count, p.like_count, u.id, u.username 
             FROM posts p INNER JOIN users u ON p.created_by = u.id 
             WHERE p.published = true AND (p.title ILIKE $1 OR p.body ILIKE $1 OR u.username ILIKE $1)
             ORDER BY p.{} {} LIMIT $2 OFFSET $3",
            order_field, order_dir
        )
    } else {
        format!(
            "SELECT p.id, p.title, p.body, p.created_by, p.slug, p.photo_url, p.created_at, p.updated_at, p.deleted_at, p.published, p.view_count, p.like_count, u.id, u.username 
             FROM posts p INNER JOIN users u ON p.created_by = u.id 
             WHERE p.published = true ORDER BY p.{} {} LIMIT $1 OFFSET $2",
            order_field, order_dir
        )
    };

    let rows = if let Some(ref search_val) = search_param {
        client.query(&query, &[search_val, &limit, &offset]).await?
    } else {
        client.query(&query, &[&limit, &offset]).await?
    };

    let mut posts: Vec<Post> = rows.iter().map(Post::from).collect();
    fetch_tags_for_posts(client, &mut posts).await?;

    Ok((posts, total))
}

pub async fn get_random_posts(
    client: &Client,
    limit: i64,
) -> Result<Vec<Post>, tokio_postgres::Error> {
    let rows = client
        .query(
            "SELECT p.id, p.title, p.body, p.created_by, p.slug, p.photo_url, p.created_at, p.updated_at, p.deleted_at, p.published, p.view_count, p.like_count, u.id, u.username 
             FROM posts p INNER JOIN users u ON p.created_by = u.id 
             WHERE p.published = true ORDER BY RANDOM() LIMIT $1",
            &[&limit],
        )
        .await?;

    let mut posts: Vec<Post> = rows.iter().map(Post::from).collect();
    fetch_tags_for_posts(client, &mut posts).await?;

    Ok(posts)
}

pub async fn get_post_by_username_and_slug(
    client: &Client,
    username: &str,
    slug: &str,
) -> Result<Option<Post>, tokio_postgres::Error> {
    let row = client
        .query_opt(
            "SELECT p.id, p.title, p.body, p.created_by, p.slug, p.photo_url, p.created_at, p.updated_at, p.deleted_at, p.published, p.view_count, p.like_count, u.id, u.username 
             FROM posts p 
             INNER JOIN users u ON p.created_by = u.id 
             WHERE u.username = $1 AND p.slug = $2 AND p.published = true",
            &[&username, &slug],
        )
        .await?;

    match row {
        Some(row) => {
            let mut post = Post::from_full(&row);

            // Fetch tags for this post
            let tag_rows = client
                .query(
                    "SELECT t.id, t.name, t.created_at 
                     FROM tags t 
                     INNER JOIN posts_to_tags ptt ON t.id = ptt.tag_id 
                     WHERE ptt.post_id = $1 
                     ORDER BY t.name",
                    &[&post.id],
                )
                .await?;

            let tags: Vec<Tag> = tag_rows.iter().map(Tag::from).collect();
            post.tags = tags;

            Ok(Some(post))
        }
        None => Ok(None),
    }
}

pub async fn get_posts_by_tag(
    client: &Client,
    tag_name: &str,
    offset: i64,
    limit: i64,
    search: Option<&str>,
    order_by: Option<&str>,
    order_direction: Option<&crate::handlers::OrderDirection>,
) -> Result<(Vec<Post>, i64), tokio_postgres::Error> {
    let order_field = validate_order_field(order_by);
    let order_dir = get_order_dir(order_direction);
    let search_param = search.map(|s| format!("%{}%", escape_like_pattern(s)));

    // Get total count
    let total: i64 = if let Some(ref search_val) = search_param {
        client
            .query_one(
                "SELECT COUNT(DISTINCT p.id) FROM posts p
                 INNER JOIN users u ON p.created_by = u.id
                 INNER JOIN posts_to_tags ptt ON p.id = ptt.post_id
                 INNER JOIN tags t ON ptt.tag_id = t.id
                 WHERE t.name = $1 AND p.published = true AND (p.title ILIKE $2 OR p.body ILIKE $2 OR u.username ILIKE $2)",
                &[&tag_name, search_val],
            )
            .await?
            .get(0)
    } else {
        client
            .query_one(
                "SELECT COUNT(DISTINCT p.id) FROM posts p
                 INNER JOIN posts_to_tags ptt ON p.id = ptt.post_id
                 INNER JOIN tags t ON ptt.tag_id = t.id
                 WHERE t.name = $1 AND p.published = true",
                &[&tag_name],
            )
            .await?
            .get(0)
    };

    // Build and execute main query
    let query = if search_param.is_some() {
        format!(
            "SELECT DISTINCT p.id, p.title, p.body, p.created_by, p.slug, p.photo_url, p.created_at, p.updated_at, p.deleted_at, p.published, p.view_count, p.like_count, u.id, u.username
             FROM posts p INNER JOIN users u ON p.created_by = u.id
             INNER JOIN posts_to_tags ptt ON p.id = ptt.post_id
             INNER JOIN tags t ON ptt.tag_id = t.id
             WHERE t.name = $1 AND p.published = true AND (p.title ILIKE $2 OR p.body ILIKE $2 OR u.username ILIKE $2)
             ORDER BY p.{} {} LIMIT $3 OFFSET $4",
            order_field, order_dir
        )
    } else {
        format!(
            "SELECT DISTINCT p.id, p.title, p.body, p.created_by, p.slug, p.photo_url, p.created_at, p.updated_at, p.deleted_at, p.published, p.view_count, p.like_count, u.id, u.username
             FROM posts p INNER JOIN users u ON p.created_by = u.id
             INNER JOIN posts_to_tags ptt ON p.id = ptt.post_id
             INNER JOIN tags t ON ptt.tag_id = t.id
             WHERE t.name = $1 AND p.published = true ORDER BY p.{} {} LIMIT $2 OFFSET $3",
            order_field, order_dir
        )
    };

    let rows = if let Some(ref search_val) = search_param {
        client
            .query(&query, &[&tag_name, search_val, &limit, &offset])
            .await?
    } else {
        client.query(&query, &[&tag_name, &limit, &offset]).await?
    };

    let mut posts: Vec<Post> = rows.iter().map(Post::from).collect();
    fetch_tags_for_posts(client, &mut posts).await?;

    Ok((posts, total))
}
