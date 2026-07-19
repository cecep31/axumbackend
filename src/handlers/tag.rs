use crate::auth::{AdminUser, AuthUser};
use crate::database::DbPool;
use crate::dto::tag::{CreateTagRequest, TagIdPath, UpdateTagRequest};
use crate::error::AppError;
use crate::models::tag::{SitemapTag, Tag, TrendingTag};
use crate::response::ApiResponse;
use crate::services;
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};
use axum_valid::Valid;

const TRENDING_TAGS_LIMIT: i64 = 5;

pub async fn get_tags(State(pool): State<DbPool>) -> Result<Json<ApiResponse<Vec<Tag>>>, AppError> {
    let tags = services::tag::get_all_tags(&pool).await?;
    Ok(Json(ApiResponse::success_with_message(
        "Successfully retrieved tags",
        tags,
    )))
}

pub async fn get_tags_for_sitemap(
    State(pool): State<DbPool>,
) -> Result<Json<ApiResponse<Vec<SitemapTag>>>, AppError> {
    let client = pool;
    let tags = services::tag::get_tags_for_sitemap(&client, 1000).await?;
    Ok(Json(ApiResponse::success_with_message(
        "Successfully retrieved tags for sitemap",
        tags,
    )))
}

pub async fn get_trending_tags(
    State(pool): State<DbPool>,
) -> Result<Json<ApiResponse<Vec<TrendingTag>>>, AppError> {
    let tags = services::tag::get_trending_tags(&pool, TRENDING_TAGS_LIMIT).await?;
    Ok(Json(ApiResponse::success_with_message(
        "Successfully retrieved trending tags",
        tags,
    )))
}

pub async fn create_tag(
    State(pool): State<DbPool>,
    _auth_user: AuthUser,
    Valid(Json(req)): Valid<Json<CreateTagRequest>>,
) -> Result<(axum::http::StatusCode, Json<ApiResponse<Tag>>), AppError> {
    let tag = services::tag::create_tag(&pool, req.name).await?;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(ApiResponse::success_with_message(
            "Tag created successfully",
            tag,
        )),
    ))
}

pub async fn get_tag_by_id(
    State(pool): State<DbPool>,
    Valid(Path(params)): Valid<Path<TagIdPath>>,
) -> Result<Json<ApiResponse<Tag>>, AppError> {
    let client = pool;
    match services::tag::get_tag_by_id(&client, params.id).await {
        Ok(Some(tag)) => Ok(Json(ApiResponse::success_with_message(
            "Successfully retrieved tag",
            tag,
        ))),
        Ok(None) => Err(AppError::NotFound("Tag not found".to_string())),
        Err(e) => Err(AppError::from(e)),
    }
}

pub async fn update_tag(
    State(pool): State<DbPool>,
    _admin_user: AdminUser,
    Valid(Path(params)): Valid<Path<TagIdPath>>,
    Valid(Json(req)): Valid<Json<UpdateTagRequest>>,
) -> Result<Json<ApiResponse<Tag>>, AppError> {
    match services::tag::update_tag(&pool, params.id, req.name).await? {
        Some(tag) => Ok(Json(ApiResponse::success_with_message(
            "Successfully updated tag",
            tag,
        ))),
        None => Err(AppError::NotFound("Tag not found".to_string())),
    }
}

pub async fn delete_tag(
    State(pool): State<DbPool>,
    _admin_user: AdminUser,
    Valid(Path(params)): Valid<Path<TagIdPath>>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    match services::tag::delete_tag(&pool, params.id).await? {
        true => Ok(Json(ApiResponse::success_with_message(
            "Successfully deleted tag",
            serde_json::Value::Null,
        ))),
        false => Err(AppError::NotFound("Tag not found".to_string())),
    }
}

pub fn routes() -> Router<DbPool> {
    Router::new()
        .route("/api/tags", get(get_tags).post(create_tag))
        .route("/api/tags/trending", get(get_trending_tags))
        .route("/api/tags/sitemap", get(get_tags_for_sitemap))
        .route(
            "/api/tags/{id}",
            get(get_tag_by_id).put(update_tag).delete(delete_tag),
        )
}
