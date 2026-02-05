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
use axum_valid::Valid;
use serde::Deserialize;
use validator::Validate;

#[derive(Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct TagPaginationQuery {
    #[validate(range(min = 0, max = 10_000))]
    offset: Option<i64>,
    #[validate(range(min = 1, max = 100))]
    limit: Option<i64>,
}

pub async fn get_tags(
    State(pool): State<DbPool>,
    Valid(query): Valid<Query<TagPaginationQuery>>,
) -> Result<Json<ApiResponse<Vec<Tag>>>, AppError> {
    let client = pool.get().await?;
    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(50);
    
    let (tags, total) = services::tag::get_all_tags(&client, offset, limit).await?;
    Ok(Json(ApiResponse::with_meta(tags, total, limit, offset)))
}

pub fn routes() -> Router<DbPool> {
    Router::new().route("/v1/tags", get(get_tags))
}
