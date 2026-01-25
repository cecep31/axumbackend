use crate::database::DbPool;
use crate::error::AppError;
use crate::models::response::ApiResponse;
use crate::models::tag::Tag;
use crate::services;
use axum::{Json, Router, extract::State, routing::get};

pub async fn get_tags(State(pool): State<DbPool>) -> Result<Json<ApiResponse<Vec<Tag>>>, AppError> {
    let client = pool.get().await?;
    let tags = services::tag::get_all_tags(&client).await?;
    let total = tags.len() as i64;
    Ok(Json(ApiResponse::with_meta(tags, total, total, 0)))
}

pub fn routes() -> Router<DbPool> {
    Router::new().route("/v1/tags", get(get_tags))
}
