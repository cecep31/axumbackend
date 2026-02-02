use crate::database::DbPool;
use crate::error::AppError;
use crate::models::tag::Tag;
use crate::response::ApiResponse;
use crate::services;
use axum::{
    Json, Router,
    extract::{Query, State},
    routing::get,
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagPaginationQuery {
    offset: Option<i64>,
    limit: Option<i64>,
}

pub async fn get_tags(
    State(pool): State<DbPool>,
    query: Query<TagPaginationQuery>,
) -> Result<Json<ApiResponse<Vec<Tag>>>, AppError> {
    let client = pool.get().await?;
    let offset = query.offset.unwrap_or(0).max(0);
    let limit = query.limit.unwrap_or(50).clamp(1, 100);
    
    let (tags, total) = services::tag::get_all_tags(&client, offset, limit).await?;
    Ok(Json(ApiResponse::with_meta(tags, total, limit, offset)))
}

pub fn routes() -> Router<DbPool> {
    Router::new().route("/v1/tags", get(get_tags))
}
