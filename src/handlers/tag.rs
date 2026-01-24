use crate::error::AppError;
use crate::models::response::ApiResponse;
use crate::models::tag::Tag;
use crate::services;
use axum::{extract::State, routing::get, Json, Router};
use deadpool_postgres::Pool;
use std::sync::Arc;

pub async fn get_tags(State(pool): State<Arc<Pool>>) -> Result<Json<ApiResponse<Vec<Tag>>>, AppError> {
    let conn = pool.get().await.map_err(|e| {
        tracing::error!("Failed to get connection from pool: {}", e);
        AppError::Database(e.to_string())
    })?;
    
    let tags = services::tag::get_all_tags(&conn).await.unwrap_or_else(|_| vec![]);
    let total = tags.len() as i64;
    Ok(Json(ApiResponse::with_meta(tags, total, None, None)))
}

pub fn routes() -> Router<Arc<Pool>> {
    Router::new().route("/v1/tags", get(get_tags))
}
