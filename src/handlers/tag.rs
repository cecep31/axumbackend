use crate::dto::stats::StatsQuery;
use crate::dto::tag::{CreateTagRequest, UpdateTagRequest, parse_tag_id};
use crate::http::auth::{AdminUser, AuthUser};
use crate::http::error::AppError;
use crate::http::extract::{VBind, VQuery, bind_message, raw_path_param};
use crate::http::response::ApiResponse;
use crate::infra::database::DbPool;
use crate::models::stats::TagPerformance;
use crate::models::tag::{SitemapTag, Tag, TrendingTag};
use crate::services;
use crate::state::AppState;
use axum::{
    Json, Router,
    extract::{FromRequestParts, State},
    http::request::Parts,
    routing::get,
};

const TRENDING_TAGS_LIMIT: i64 = 5;

/// echobackend's `c.Bind` failure message for the tag bodies.
const BIND_MESSAGE: &str = "Invalid request payload";

/// The `{id}` tag id, parsed like echobackend's `strconv.ParseUint(c.Param
/// ("id"), 10, 32)`: 400 `{message: "Invalid tag ID", error: <ParseUint
/// error>}` otherwise. Declared after the auth extractor and before the body.
pub struct TagIdParam(pub u32);

impl<S: Send + Sync> FromRequestParts<S> for TagIdParam {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let raw = raw_path_param(parts, state, "id").await.unwrap_or_default();
        parse_tag_id(&raw)
            .map(TagIdParam)
            .map_err(|err| AppError::bad_request("Invalid tag ID", Some(err)))
    }
}

/// echobackend `response.NotFound(c, "Tag not found", err)`: `err` is
/// `ErrInvalidTagID` ("invalid tag ID") for id 0, else `ErrTagNotFound`.
fn tag_not_found(id: u32) -> AppError {
    let detail = if id == 0 {
        "invalid tag ID"
    } else {
        "tag not found"
    };
    AppError::not_found("Tag not found", Some(detail.to_string()))
}

/// The `tags.id` (int4) a parsed id can name; ids past `i32::MAX` match no
/// row, like echobackend's lookup.
fn db_tag_id(id: u32) -> Option<i32> {
    i32::try_from(id).ok().filter(|id| *id > 0)
}

pub async fn get_tags(State(pool): State<DbPool>) -> Result<Json<ApiResponse<Vec<Tag>>>, AppError> {
    let tags = services::tag::get_all_tags(&pool)
        .await
        .map_err(|e| AppError::from(e).context("Failed to get tags"))?;
    Ok(Json(ApiResponse::success_with_message(
        "Successfully retrieved tags",
        tags,
    )))
}

pub async fn get_tags_for_sitemap(
    State(pool): State<DbPool>,
) -> Result<Json<ApiResponse<Vec<SitemapTag>>>, AppError> {
    let client = pool;
    let tags = services::tag::get_tags_for_sitemap(&client, 1000)
        .await
        .map_err(|e| AppError::from(e).context("Failed to get tags for sitemap"))?;
    Ok(Json(ApiResponse::success_with_message(
        "Successfully retrieved tags for sitemap",
        tags,
    )))
}

pub async fn get_trending_tags(
    State(pool): State<DbPool>,
) -> Result<Json<ApiResponse<Vec<TrendingTag>>>, AppError> {
    let tags = services::tag::get_trending_tags(&pool, TRENDING_TAGS_LIMIT)
        .await
        .map_err(|e| AppError::from(e).context("Failed to get trending tags"))?;
    Ok(Json(ApiResponse::success_with_message(
        "Successfully retrieved trending tags",
        tags,
    )))
}

pub async fn create_tag(
    State(pool): State<DbPool>,
    _auth_user: AuthUser,
    body: Result<VBind<CreateTagRequest>, AppError>,
) -> Result<(axum::http::StatusCode, Json<ApiResponse<Tag>>), AppError> {
    let VBind(req) = body.map_err(bind_message(BIND_MESSAGE))?;
    // echobackend: any create failure, a duplicate name included, is a 500.
    let tag = services::tag::create_tag(&pool, req.name)
        .await
        .map_err(|e| AppError::from(e).context("Failed to create tag"))?;
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
    TagIdParam(id): TagIdParam,
) -> Result<Json<ApiResponse<Tag>>, AppError> {
    let Some(db_id) = db_tag_id(id) else {
        return Err(tag_not_found(id));
    };
    match services::tag::get_tag_by_id(&pool, db_id).await {
        Ok(Some(tag)) => Ok(Json(ApiResponse::success_with_message(
            "Successfully retrieved tag",
            tag,
        ))),
        Ok(None) => Err(tag_not_found(id)),
        Err(e) => Err(AppError::from(e).context("Failed to get tag")),
    }
}

pub async fn update_tag(
    State(pool): State<DbPool>,
    _admin_user: AdminUser,
    TagIdParam(id): TagIdParam,
    body: Result<VBind<UpdateTagRequest>, AppError>,
) -> Result<Json<ApiResponse<Tag>>, AppError> {
    let VBind(req) = body.map_err(bind_message(BIND_MESSAGE))?;
    let Some(db_id) = db_tag_id(id) else {
        return Err(tag_not_found(id));
    };
    match services::tag::update_tag(&pool, db_id, req.name)
        .await
        .map_err(|e| AppError::from(e).context("Failed to update tag"))?
    {
        Some(tag) => Ok(Json(ApiResponse::success_with_message(
            "Tag updated successfully",
            tag,
        ))),
        None => Err(tag_not_found(id)),
    }
}

pub async fn delete_tag(
    State(pool): State<DbPool>,
    _admin_user: AdminUser,
    TagIdParam(id): TagIdParam,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let Some(db_id) = db_tag_id(id) else {
        return Err(tag_not_found(id));
    };
    match services::tag::delete_tag(&pool, db_id)
        .await
        .map_err(|e| AppError::from(e).context("Failed to delete tag"))?
    {
        true => Ok(Json(ApiResponse::message("Tag deleted successfully"))),
        false => Err(tag_not_found(id)),
    }
}

pub async fn get_tag_stats(
    State(pool): State<DbPool>,
    _admin_user: AdminUser,
    VQuery(query): VQuery<StatsQuery>,
) -> Result<Json<ApiResponse<Vec<TagPerformance>>>, AppError> {
    let stats = services::stats::tag_stats(&pool, query.limit())
        .await
        .map_err(|e| AppError::from(e).context("Failed to fetch tag stats"))?;
    Ok(Json(ApiResponse::success_with_message(
        "Tag stats fetched successfully",
        stats,
    )))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/tags", get(get_tags).post(create_tag))
        .route("/api/tags/stats", get(get_tag_stats))
        .route("/api/tags/trending", get(get_trending_tags))
        .route("/api/tags/sitemap", get(get_tags_for_sitemap))
        .route(
            "/api/tags/{id}",
            get(get_tag_by_id).put(update_tag).delete(delete_tag),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use axum::response::IntoResponse;

    async fn render(err: AppError) -> (StatusCode, serde_json::Value) {
        let response = err.into_response();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    async fn tag_id_of(uri: &str) -> Result<u32, (StatusCode, serde_json::Value)> {
        let router: Router = Router::new().route(
            "/api/tags/{id}",
            get(|TagIdParam(id): TagIdParam| async move { id.to_string() }),
        );
        let response = tower::ServiceExt::oneshot(
            router,
            axum::http::Request::builder()
                .uri(uri)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        if status == StatusCode::OK {
            Ok(String::from_utf8(bytes.to_vec()).unwrap().parse().unwrap())
        } else {
            Err((status, serde_json::from_slice(&bytes).unwrap()))
        }
    }

    #[tokio::test]
    async fn tag_id_param_like_parse_uint32() {
        assert_eq!(tag_id_of("/api/tags/0").await.unwrap(), 0);
        assert_eq!(tag_id_of("/api/tags/4294967295").await.unwrap(), u32::MAX);
        let (status, body) = tag_id_of("/api/tags/abc").await.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            body,
            serde_json::json!({
                "success": false,
                "message": "Invalid tag ID",
                "error": "strconv.ParseUint: parsing \"abc\": invalid syntax"
            })
        );
    }

    #[tokio::test]
    async fn tag_not_found_details() {
        let (status, body) = render(tag_not_found(0)).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["message"], "Tag not found");
        assert_eq!(body["error"], "invalid tag ID");
        let (_, body) = render(tag_not_found(7)).await;
        assert_eq!(body["error"], "tag not found");
        assert_eq!(db_tag_id(0), None);
        assert_eq!(db_tag_id(u32::MAX), None);
        assert_eq!(db_tag_id(7), Some(7));
    }
}
