use crate::database::DbPool;
use crate::error::AppError;
use crate::models::post::Post;
use crate::models::response::ApiResponse;
use crate::services;
use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct RandomPostQuery {
    limit: Option<i64>,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum OrderDirection {
    Asc,
    Desc,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaginationQuery {
    offset: Option<i64>,
    limit: Option<i64>,
    search: Option<String>,
    order_by: Option<String>,
    order_direction: Option<OrderDirection>,
}

pub async fn get_posts(
    State(pool): State<DbPool>,
    query: Query<PaginationQuery>,
) -> Result<Json<ApiResponse<Vec<Post>>>, AppError> {
    let client = pool.get().await?;
    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(10);
    let search = query.search.as_deref();
    let order_by = query.order_by.as_deref();
    let order_direction = query.order_direction.as_ref();

    let (posts, total) = services::post::get_all_posts(
        &client,
        offset,
        limit,
        search,
        order_by,
        order_direction,
    )
    .await
    .unwrap_or_else(|_| (vec![], 0));

    Ok(Json(ApiResponse::with_meta(posts, total, Some(limit), Some(offset))))
}

pub async fn get_random_posts(
    State(pool): State<DbPool>,
    query: Query<RandomPostQuery>,
) -> Result<Json<ApiResponse<Vec<Post>>>, AppError> {
    let client = pool.get().await?;
    let limit = query.limit.unwrap_or(6);
    let posts = services::post::get_random_posts(&client, limit)
        .await
        .unwrap_or_else(|_| vec![]);
    let total = posts.len() as i64;
    Ok(Json(ApiResponse::with_meta(posts, total, Some(limit), None)))
}

pub async fn get_posts_by_tag(
    State(pool): State<DbPool>,
    Path(tag): Path<String>,
    query: Query<PaginationQuery>,
) -> Result<Json<ApiResponse<Vec<Post>>>, AppError> {
    let client = pool.get().await?;
    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(10);
    let search = query.search.as_deref();
    let order_by = query.order_by.as_deref();
    let order_direction = query.order_direction.as_ref();

    let (posts, total) = services::post::get_posts_by_tag(
        &client,
        &tag,
        offset,
        limit,
        search,
        order_by,
        order_direction,
    )
    .await
    .unwrap_or_else(|_| (vec![], 0));

    Ok(Json(ApiResponse::with_meta(posts, total, Some(limit), Some(offset))))
}

pub async fn get_post_by_username_and_slug(
    State(pool): State<DbPool>,
    Path((username, slug)): Path<(String, String)>,
) -> Result<Json<ApiResponse<Post>>, AppError> {
    let client = pool.get().await?;
    match services::post::get_post_by_username_and_slug(&client, &username, &slug).await {
        Ok(Some(post)) => Ok(Json(ApiResponse::success(post))),
        Ok(None) => Err(AppError::NotFound(format!(
            "Post not found: {} by {}",
            slug, username
        ))),
        Err(e) => Err(AppError::from(e)),
    }
}

pub fn routes() -> Router<DbPool> {
    Router::new()
        .route("/v1/posts", get(get_posts))
        .route("/v1/posts/random", get(get_random_posts))
        .route("/v1/posts/tag/{tag}", get(get_posts_by_tag))
        .route("/v1/posts/u/{username}/{slug}", get(get_post_by_username_and_slug))
}
