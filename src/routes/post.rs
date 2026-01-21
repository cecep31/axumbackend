use crate::models::post::Post;
use crate::models::response::ApiResponse;
use crate::services;
use std::sync::Arc;
use axum::{
    extract::{Query, State},
    Json,
};
use serde::Deserialize;
use tokio_postgres::Client;

#[derive(Deserialize)]
pub struct RandomPostQuery {
    limit: Option<i64>,
}

pub async fn get_posts(State(conn): State<Arc<Client>>) -> Json<ApiResponse<Vec<Post>>> {
    let posts = services::post::get_all_posts(&conn).await.unwrap_or_else(|_| vec![]);
    let total = posts.len() as i64;
    Json(ApiResponse::with_meta(posts, total, None, None))
}

pub async fn get_random_posts(
    State(conn): State<Arc<Client>>,
    query: Query<RandomPostQuery>,
) -> Json<ApiResponse<Vec<Post>>> {
    let limit = query.limit.unwrap_or(9);
    let posts = services::post::get_random_posts(&conn, limit).await.unwrap_or_else(|_| vec![]);
    let total = posts.len() as i64;
    Json(ApiResponse::with_meta(posts, total, Some(limit), None))
}
