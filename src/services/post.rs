use crate::models::post::Post;
use crate::models::tag::Tag;
use crate::models::user::User;
use chrono::{DateTime, Utc};
use tokio_postgres::Client;
use uuid::Uuid;

pub async fn get_all_posts(
    client: &Client,
    offset: i64,
    limit: i64,
    search: Option<&str>,
    order_by: Option<&str>,
    order_direction: Option<&crate::handlers::OrderDirection>,
) -> Result<(Vec<Post>, i64), tokio_postgres::Error> {
    // Validate and sanitize order_by field
    let valid_order_fields = ["id", "title", "created_at", "updated_at", "view_count", "like_count"];
    let order_field = order_by
        .and_then(|field| {
            if valid_order_fields.contains(&field) {
                Some(field)
            } else {
                None
            }
        })
        .unwrap_or("id");
    
    let order_dir = match order_direction {
        Some(crate::handlers::OrderDirection::Desc) => "DESC",
        _ => "ASC",
    };

    // Build WHERE clause for search
    let search_param = search.map(|s| format!("%{}%", s));

    // Get total count
    let total: i64 = if let Some(ref search_val) = search_param {
        let total_row = client
            .query_one(
                "SELECT COUNT(*) FROM posts p INNER JOIN users u ON p.created_by = u.id WHERE (p.title ILIKE $1 OR p.body ILIKE $1 OR u.username ILIKE $1)",
                &[search_val],
            )
            .await?;
        total_row.get(0)
    } else {
        let total_row = client
            .query_one("SELECT COUNT(*) FROM posts", &[])
            .await?;
        total_row.get(0)
    };

    // Build main query - ORDER BY field is validated against whitelist, so safe to format
    let query = if search_param.is_some() {
        format!(
            "SELECT p.id, p.title, p.body, p.created_by, p.slug, p.photo_url, p.created_at, p.updated_at, p.deleted_at, p.published, p.view_count, p.like_count, u.id, u.username 
             FROM posts p 
             INNER JOIN users u ON p.created_by = u.id 
             WHERE (p.title ILIKE $1 OR p.body ILIKE $1 OR u.username ILIKE $1)
             ORDER BY p.{} {} 
             LIMIT $2 OFFSET $3",
            order_field, order_dir
        )
    } else {
        format!(
            "SELECT p.id, p.title, p.body, p.created_by, p.slug, p.photo_url, p.created_at, p.updated_at, p.deleted_at, p.published, p.view_count, p.like_count, u.id, u.username 
             FROM posts p 
             INNER JOIN users u ON p.created_by = u.id 
             ORDER BY p.{} {} 
             LIMIT $1 OFFSET $2",
            order_field, order_dir
        )
    };

    // Get paginated posts
    let rows = if let Some(ref search_val) = search_param {
        client.query(&query, &[search_val, &limit, &offset]).await?
    } else {
        client.query(&query, &[&limit, &offset]).await?
    };

    let posts: Result<Vec<Post>, _> = rows
        .iter()
        .map(|row| {
            let id: Uuid = row.get(0);
            let title: String = row.get(1);
            let body: Option<String> = row.get(2);
            let created_by: Uuid = row.get(3);
            let slug: String = row.get(4);
            let photo_url: Option<String> = row.get(5);
            let created_at: DateTime<Utc> = row.get(6);
            let updated_at: DateTime<Utc> = row.get(7);
            let deleted_at: Option<DateTime<Utc>> = row.get(8);
            let published: bool = row.get(9);
            let view_count: i64 = row.get(10);
            let like_count: i64 = row.get(11);
            let user_id: Uuid = row.get(12);
            let username: String = row.get(13);

            // Substring body to 200 characters max
            let body = body.map(|b| {
                if b.chars().count() > 200 {
                    let truncated_body: String = b.chars().take(200).collect();
                    format!("{}...", truncated_body)
                } else {
                    b
                }
            });

            Ok(Post {
                id,
                title,
                body,
                created_by,
                slug,
                photo_url,
                created_at,
                updated_at,
                deleted_at,
                published,
                view_count,
                like_count,
                creator: User {
                    id: user_id,
                    username,
                },
                tags: Vec::new(),
            })
        })
        .collect();

    posts.map(|posts| (posts, total))
}

pub async fn get_random_posts(client: &Client, limit: i64) -> Result<Vec<Post>, tokio_postgres::Error> {
    let rows = client.query(
        "SELECT p.id, p.title, p.body, p.created_by, p.slug, p.photo_url, p.created_at, p.updated_at, p.deleted_at, p.published, p.view_count, p.like_count, u.id, u.username 
         FROM posts p 
         INNER JOIN users u ON p.created_by = u.id 
         ORDER BY RANDOM() 
         LIMIT $1",
        &[&limit]
    ).await?;

    let posts: Result<Vec<Post>, _> = rows.iter().map(|row| {
        let id: Uuid = row.get(0);
        let title: String = row.get(1);
        let body: Option<String> = row.get(2);
        let created_by: Uuid = row.get(3);
        let slug: String = row.get(4);
        let photo_url: Option<String> = row.get(5);
        let created_at: DateTime<Utc> = row.get(6);
        let updated_at: DateTime<Utc> = row.get(7);
        let deleted_at: Option<DateTime<Utc>> = row.get(8);
        let published: bool = row.get(9);
        let view_count: i64 = row.get(10);
        let like_count: i64 = row.get(11);
        let user_id: Uuid = row.get(12);
        let username: String = row.get(13);

        // Substring body to 200 characters max
        let body = body.map(|b| {
            if b.chars().count() > 200 {
                let truncated_body: String = b.chars().take(200).collect();
                format!("{}...", truncated_body)
            } else {
                b
            }
        });

        Ok(Post {
            id,
            title,
            body,
            created_by,
            slug,
            photo_url,
            created_at,
            updated_at,
            deleted_at,
            published,
            view_count,
            like_count,
            creator: User {
                id: user_id,
                username,
            },
            tags: Vec::new(),
        })
    }).collect();

    posts
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
             WHERE u.username = $1 AND p.slug = $2",
            &[&username, &slug],
        )
        .await?;

    match row {
        Some(row) => {
            let id: Uuid = row.get(0);
            let title: String = row.get(1);
            let body: Option<String> = row.get(2);
            let created_by: Uuid = row.get(3);
            let slug: String = row.get(4);
            let photo_url: Option<String> = row.get(5);
            let created_at: DateTime<Utc> = row.get(6);
            let updated_at: DateTime<Utc> = row.get(7);
            let deleted_at: Option<DateTime<Utc>> = row.get(8);
            let published: bool = row.get(9);
            let view_count: i64 = row.get(10);
            let like_count: i64 = row.get(11);
            let user_id: Uuid = row.get(12);
            let username: String = row.get(13);

            // Fetch tags for this post
            let tag_rows = client
                .query(
                    "SELECT t.id, t.name 
                     FROM tags t 
                     INNER JOIN posts_to_tags ptt ON t.id = ptt.tag_id 
                     WHERE ptt.post_id = $1 
                     ORDER BY t.name",
                    &[&id],
                )
                .await?;

            let tags: Vec<Tag> = tag_rows
                .iter()
                .map(|tag_row| {
                    let tag_id: Uuid = tag_row.get(0);
                    let tag_name: String = tag_row.get(1);
                    Tag {
                        id: tag_id,
                        name: tag_name,
                    }
                })
                .collect();

            Ok(Some(Post {
                id,
                title,
                body,
                created_by,
                slug,
                photo_url,
                created_at,
                updated_at,
                deleted_at,
                published,
                view_count,
                like_count,
                creator: User {
                    id: user_id,
                    username,
                },
                tags,
            }))
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
    // Validate and sanitize order_by field
    let valid_order_fields = ["id", "title", "created_at", "updated_at", "view_count", "like_count"];
    let order_field = order_by
        .and_then(|field| {
            if valid_order_fields.contains(&field) {
                Some(field)
            } else {
                None
            }
        })
        .unwrap_or("id");
    
    let order_dir = match order_direction {
        Some(crate::handlers::OrderDirection::Desc) => "DESC",
        _ => "ASC",
    };

    // Build WHERE clause for search
    let search_param = search.map(|s| format!("%{}%", s));

    // Get total count
    let total: i64 = if let Some(ref search_val) = search_param {
        let total_row = client
            .query_one(
                "SELECT COUNT(DISTINCT p.id) 
                 FROM posts p 
                 INNER JOIN users u ON p.created_by = u.id 
                 INNER JOIN posts_to_tags ptt ON p.id = ptt.post_id 
                 INNER JOIN tags t ON ptt.tag_id = t.id 
                 WHERE t.name = $1 AND (p.title ILIKE $2 OR p.body ILIKE $2 OR u.username ILIKE $2)",
                &[&tag_name, search_val],
            )
            .await?;
        total_row.get(0)
    } else {
        let total_row = client
            .query_one(
                "SELECT COUNT(DISTINCT p.id) 
                 FROM posts p 
                 INNER JOIN posts_to_tags ptt ON p.id = ptt.post_id 
                 INNER JOIN tags t ON ptt.tag_id = t.id 
                 WHERE t.name = $1",
                &[&tag_name],
            )
            .await?;
        total_row.get(0)
    };

    // Build main query
    let query = if search_param.is_some() {
        format!(
            "SELECT DISTINCT p.id, p.title, p.body, p.created_by, p.slug, p.photo_url, p.created_at, p.updated_at, p.deleted_at, p.published, p.view_count, p.like_count, u.id, u.username 
             FROM posts p 
             INNER JOIN users u ON p.created_by = u.id 
             INNER JOIN posts_to_tags ptt ON p.id = ptt.post_id 
             INNER JOIN tags t ON ptt.tag_id = t.id 
             WHERE t.name = $1 AND (p.title ILIKE $2 OR p.body ILIKE $2 OR u.username ILIKE $2)
             ORDER BY p.{} {} 
             LIMIT $3 OFFSET $4",
            order_field, order_dir
        )
    } else {
        format!(
            "SELECT DISTINCT p.id, p.title, p.body, p.created_by, p.slug, p.photo_url, p.created_at, p.updated_at, p.deleted_at, p.published, p.view_count, p.like_count, u.id, u.username 
             FROM posts p 
             INNER JOIN users u ON p.created_by = u.id 
             INNER JOIN posts_to_tags ptt ON p.id = ptt.post_id 
             INNER JOIN tags t ON ptt.tag_id = t.id 
             WHERE t.name = $1
             ORDER BY p.{} {} 
             LIMIT $2 OFFSET $3",
            order_field, order_dir
        )
    };

    // Get paginated posts
    let rows = if let Some(ref search_val) = search_param {
        client.query(&query, &[&tag_name, search_val, &limit, &offset]).await?
    } else {
        client.query(&query, &[&tag_name, &limit, &offset]).await?
    };

    // Fetch posts and their tags
    let mut posts = Vec::new();
    for row in rows {
        let id: Uuid = row.get(0);
        let title: String = row.get(1);
        let body: Option<String> = row.get(2);
        let created_by: Uuid = row.get(3);
        let slug: String = row.get(4);
        let photo_url: Option<String> = row.get(5);
        let created_at: DateTime<Utc> = row.get(6);
        let updated_at: DateTime<Utc> = row.get(7);
        let deleted_at: Option<DateTime<Utc>> = row.get(8);
        let published: bool = row.get(9);
        let view_count: i64 = row.get(10);
        let like_count: i64 = row.get(11);
        let user_id: Uuid = row.get(12);
        let username: String = row.get(13);

        // Substring body to 200 characters max
        let body = body.map(|b| {
            if b.chars().count() > 200 {
                let truncated_body: String = b.chars().take(200).collect();
                format!("{}...", truncated_body)
            } else {
                b
            }
        });

        // Fetch tags for this post
        let tag_rows = client
            .query(
                "SELECT t.id, t.name 
                 FROM tags t 
                 INNER JOIN posts_to_tags ptt ON t.id = ptt.tag_id 
                 WHERE ptt.post_id = $1 
                 ORDER BY t.name",
                &[&id],
            )
            .await?;

        let tags: Vec<Tag> = tag_rows
            .iter()
            .map(|tag_row| {
                let tag_id: Uuid = tag_row.get(0);
                let tag_name: String = tag_row.get(1);
                Tag {
                    id: tag_id,
                    name: tag_name,
                }
            })
            .collect();

        posts.push(Post {
            id,
            title,
            body,
            created_by,
            slug,
            photo_url,
            created_at,
            updated_at,
            deleted_at,
            published,
            view_count,
            like_count,
            creator: User {
                id: user_id,
                username,
            },
            tags,
        });
    }

    Ok((posts, total))
}
