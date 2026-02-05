use crate::database::DbPool;
use crate::error::AppError;
use crate::models::post::Post;
use crate::response::ApiResponse;
use crate::services;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::get,
};
use axum_valid::Valid;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Deserialize;
use validator::Validate;

#[derive(Deserialize, Validate)]
pub struct RandomPostQuery {
    #[validate(range(min = 1, max = 100))]
    limit: Option<i64>,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum OrderDirection {
    Asc,
    Desc,
}

#[derive(Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct PaginationQuery {
    #[validate(range(min = 0, max = 10_000))]
    offset: Option<i64>,
    #[validate(range(min = 1, max = 100))]
    limit: Option<i64>,
    #[validate(length(max = 200))]
    search: Option<String>,
    order_by: Option<String>,
    order_direction: Option<OrderDirection>,
}

static USERNAME_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[a-zA-Z0-9_-]+$").unwrap());
static SLUG_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[a-zA-Z0-9-]+$").unwrap());
static TAG_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[a-zA-Z0-9_-]+$").unwrap());

fn get_pagination_params(
    query: &PaginationQuery,
) -> (
    i64,
    i64,
    Option<&str>,
    Option<&str>,
    Option<&OrderDirection>,
) {
    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(10);
    let search = query.search.as_deref();
    let order_by = query.order_by.as_deref();
    let order_direction = query.order_direction.as_ref();
    (offset, limit, search, order_by, order_direction)
}

pub async fn get_posts(
    State(pool): State<DbPool>,
    Valid(query): Valid<Query<PaginationQuery>>,
) -> Result<Json<ApiResponse<Vec<Post>>>, AppError> {
    let client = pool.get().await?;
    let (offset, limit, search, order_by, order_direction) = get_pagination_params(&query);

    let (posts, total) =
        services::post::get_all_posts(&client, offset, limit, search, order_by, order_direction)
            .await?;

    Ok(Json(ApiResponse::with_meta(posts, total, limit, offset)))
}

pub async fn get_random_posts(
    State(pool): State<DbPool>,
    Valid(query): Valid<Query<RandomPostQuery>>,
) -> Result<Json<ApiResponse<Vec<Post>>>, AppError> {
    let client = pool.get().await?;
    let limit = query.limit.unwrap_or(6);
    let posts = services::post::get_random_posts(&client, limit).await?;
    let total = posts.len() as i64;
    Ok(Json(ApiResponse::with_meta(posts, total, limit, 0)))
}

#[derive(Deserialize, Validate)]
pub struct TagPath {
    #[validate(length(min = 1, max = 50), regex(path = *TAG_RE))]
    pub tag: String,
}

pub async fn get_posts_by_tag(
    State(pool): State<DbPool>,
    Valid(Path(tag_path)): Valid<Path<TagPath>>,
    Valid(query): Valid<Query<PaginationQuery>>,
) -> Result<Json<ApiResponse<Vec<Post>>>, AppError> {
    let client = pool.get().await?;
    let (offset, limit, search, order_by, order_direction) = get_pagination_params(&query);

    let (posts, total) = services::post::get_posts_by_tag(
        &client,
        &tag_path.tag,
        offset,
        limit,
        search,
        order_by,
        order_direction,
    )
    .await?;

    Ok(Json(ApiResponse::with_meta(posts, total, limit, offset)))
}

#[derive(Deserialize, Validate)]
pub struct PostPath {
    #[validate(length(min = 1, max = 50), regex(path = *USERNAME_RE))]
    pub username: String,
    #[validate(length(min = 1, max = 100), regex(path = *SLUG_RE))]
    pub slug: String,
}

pub async fn get_post_by_username_and_slug(
    State(pool): State<DbPool>,
    Valid(Path(params)): Valid<Path<PostPath>>,
) -> Result<Json<ApiResponse<Post>>, AppError> {
    let client = pool.get().await?;
    match services::post::get_post_by_username_and_slug(&client, &params.username, &params.slug).await {
        Ok(Some(post)) => Ok(Json(ApiResponse::success(post))),
        Ok(None) => Err(AppError::NotFound(format!(
            "Post not found: {} by {}",
            params.slug, params.username
        ))),
        Err(e) => Err(AppError::from(e)),
    }
}

pub fn routes() -> Router<DbPool> {
    Router::new()
        .route("/v1/posts", get(get_posts))
        .route("/v1/posts/random", get(get_random_posts))
        .route("/v1/posts/tag/{tag}", get(get_posts_by_tag))
        .route(
            "/v1/posts/u/{username}/{slug}",
            get(get_post_by_username_and_slug),
        )
}
