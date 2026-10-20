use crate::dto::common::{PostIdPath, UsernamePath};
use crate::dto::post::{
    CreatePostRequest, PostPaginationQuery, PostPath, PostsFilterQuery, RandomPostQuery, TagPath,
    UpdatePostRequest, post_pagination_params,
};
use crate::http::auth::{AdminUser, AuthUser};
use crate::http::error::AppError;
use crate::http::extract::{VJson, VPath, VQuery};
use crate::http::response::ApiResponse;
use crate::infra::database::DbPool;
use crate::models::post::{Post, SitemapPost};
use crate::services;
use axum::{
    Json,
    extract::{Multipart, State},
};
use uuid::Uuid;

use super::{MAX_POST_IMAGE_SIZE, POST_IMAGE_PREFIX};

impl From<services::post::PostWriteError> for AppError {
    fn from(err: services::post::PostWriteError) -> Self {
        match err {
            services::post::PostWriteError::SpamRejected(reason) => {
                AppError::spam_rejected("Post rejected by anti-spam policy", "body", reason)
            }
            services::post::PostWriteError::Db(e) => AppError::from(e),
        }
    }
}

pub async fn create_post(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VJson(req): VJson<CreatePostRequest>,
) -> Result<(axum::http::StatusCode, Json<ApiResponse<serde_json::Value>>), AppError> {
    let post = services::post::create_post(
        &pool,
        services::post::CreatePostInput {
            title: req.title,
            photo_url: req.photo_url,
            slug: req.slug,
            body: req.body,
            published: req.published,
            tags: req.tags,
        },
        auth_user.id,
    )
    .await?;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(ApiResponse::success_with_message(
            "Successfully created post",
            serde_json::json!({ "id": post.id }),
        )),
    ))
}

async fn ensure_author(pool: &DbPool, post_id: Uuid, auth_user: &AuthUser) -> Result<(), AppError> {
    match services::post::is_author(pool, post_id, auth_user.id).await? {
        Some(true) => Ok(()),
        Some(false) => Err(AppError::Forbidden(
            "You are not the post author".to_string(),
        )),
        None => Err(AppError::NotFound("Post not found".to_string())),
    }
}

/// Parses a `YYYY-MM-DD` query value. Returns `400` on a malformed date.
fn parse_date_param(raw: Option<&str>, field: &str) -> Result<Option<chrono::NaiveDate>, AppError> {
    match raw.map(str::trim).filter(|value| !value.is_empty()) {
        None => Ok(None),
        Some(value) => chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
            .map(Some)
            .map_err(|_| {
                AppError::BadRequest(format!("Invalid {field} format, expected YYYY-MM-DD"))
            }),
    }
}

pub async fn get_posts(
    State(pool): State<DbPool>,
    VQuery(query): VQuery<PostsFilterQuery>,
) -> Result<Json<ApiResponse<Vec<Post>>>, AppError> {
    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(10);

    let published = match query.published.as_deref().map(str::trim) {
        None | Some("") => None,
        Some("true") => Some(true),
        Some("false") => Some(false),
        Some(_) => {
            return Err(AppError::BadRequest(
                "Invalid published value, expected true or false".to_string(),
            ));
        }
    };
    let start_date = parse_date_param(query.start_date.as_deref(), "start_date")?;
    let end_date = parse_date_param(query.end_date.as_deref(), "end_date")?;
    let created_by = match query.created_by.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(raw) => Some(Uuid::parse_str(raw).map_err(|_| {
            AppError::BadRequest("Invalid created_by value, expected UUID".to_string())
        })?),
    };
    let tags: Vec<String> = query
        .tags
        .unwrap_or_default()
        .split(',')
        .map(|tag| tag.trim().to_string())
        .filter(|tag| !tag.is_empty())
        .collect();
    // echobackend's `GetSortOrder`: anything other than asc/desc falls back to
    // the default (desc), represented by `None` here.
    let sort_order = match query.sort_order.as_deref() {
        Some("asc") => Some(services::post::SortDirection::Asc),
        Some("desc") => Some(services::post::SortDirection::Desc),
        _ => None,
    };

    let (posts, total) = services::post::get_all_posts(
        &pool,
        services::post::PostsFilter {
            offset,
            limit,
            search: query.search.as_deref(),
            sort_by: query.sort_by.as_deref(),
            sort_order,
            start_date,
            end_date,
            created_by,
            published,
            tags,
        },
    )
    .await?;

    Ok(Json(ApiResponse::with_meta_message(
        "Successfully retrieved posts",
        posts,
        total,
        limit,
        offset,
    )))
}

pub async fn get_random_posts(
    State(pool): State<DbPool>,
    VQuery(query): VQuery<RandomPostQuery>,
) -> Result<Json<ApiResponse<Vec<Post>>>, AppError> {
    let client = pool;
    let limit = query.limit.unwrap_or(9);
    let limit = limit.min(20);
    let posts = services::post::get_random_posts(&client, limit).await?;
    Ok(Json(ApiResponse::success_with_message(
        "Successfully retrieved posts",
        posts,
    )))
}

pub async fn get_trending_posts(
    State(pool): State<DbPool>,
    VQuery(query): VQuery<RandomPostQuery>,
) -> Result<Json<ApiResponse<Vec<Post>>>, AppError> {
    let client = pool;
    let limit = query.limit.unwrap_or(10).min(100);
    let posts = services::post::get_trending_posts(&client, limit).await?;
    Ok(Json(ApiResponse::success_with_message(
        "Successfully retrieved trending posts",
        posts,
    )))
}

pub async fn get_posts_for_you(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VQuery(query): VQuery<PostPaginationQuery>,
) -> Result<Json<ApiResponse<Vec<Post>>>, AppError> {
    let (offset, limit, _search, _order_by, _order_direction) = post_pagination_params(&query);
    let (posts, total) =
        services::post::get_posts_for_you(&pool, auth_user.id, offset, limit).await?;

    Ok(Json(ApiResponse::with_meta_message(
        "Successfully retrieved posts",
        posts,
        total,
        limit,
        offset,
    )))
}

/// Returns `(content_type, extension)` for allowed post images.
fn detect_allowed_image(data: &[u8]) -> Option<(&'static str, &'static str)> {
    if data.len() >= 3 && data[0..3] == [0xff, 0xd8, 0xff] {
        Some(("image/jpeg", ".jpg"))
    } else if data.len() >= 8 && data[0..8] == [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a] {
        Some(("image/png", ".png"))
    } else if data.len() >= 12 && &data[0..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        Some(("image/webp", ".webp"))
    } else {
        None
    }
}

pub async fn upload_image_posts(
    _auth_user: AuthUser,
    headers: axum::http::HeaderMap,
    mut multipart: Multipart,
) -> Result<(axum::http::StatusCode, Json<ApiResponse<serde_json::Value>>), AppError> {
    // Fail fast on oversized bodies: without this the route's body limit
    // kills the stream mid-field and multer reports a generic "Error parsing
    // multipart/form-data request" instead of a clear 413.
    crate::http::upload::reject_oversized_body(&headers, MAX_POST_IMAGE_SIZE, "Image")?;

    while let Some(field) = multipart.next_field().await.map_err(|err| {
        tracing::warn!(error = ?err, "post image: multipart field error");
        AppError::BadRequest(format!("Failed to upload image: {err}"))
    })? {
        if field.name() != Some("image") {
            continue;
        }

        let data = field.bytes().await.map_err(|err| {
            tracing::warn!(error = ?err, "post image: multipart read error");
            AppError::BadRequest(format!("Failed to read image: {err}"))
        })?;
        if data.len() as u64 > MAX_POST_IMAGE_SIZE {
            return Err(AppError::PayloadTooLarge(
                "Image is too large (max 1 MB)".to_string(),
            ));
        }
        let Some((content_type, ext)) = detect_allowed_image(&data) else {
            return Err(AppError::BadRequest("Invalid file type".to_string()));
        };
        let Some(storage) = crate::infra::storage::get() else {
            return Err(AppError::BadRequest(
                "Storage is not configured".to_string(),
            ));
        };

        let key = crate::infra::storage::random_object_key(POST_IMAGE_PREFIX, ext);
        storage
            .save(&key, data.to_vec(), content_type)
            .await
            .map_err(|err| {
                tracing::error!(error = %err, key = %key, "post image: upload failed");
                AppError::InternalServerError("Failed to upload image".to_string())
            })?;

        return Ok((
            axum::http::StatusCode::CREATED,
            Json(ApiResponse::success_with_message(
                "Successfully uploaded image",
                serde_json::json!({ "path": key, "url": key }),
            )),
        ));
    }

    Err(AppError::BadRequest("No file uploaded".to_string()))
}

pub async fn get_posts_for_sitemap(
    State(pool): State<DbPool>,
) -> Result<Json<ApiResponse<Vec<SitemapPost>>>, AppError> {
    let client = pool;
    let posts = services::post::get_posts_for_sitemap(&client, 1000).await?;
    Ok(Json(ApiResponse::success_with_message(
        "Successfully retrieved posts for sitemap",
        posts,
    )))
}

pub async fn get_posts_by_tag(
    State(pool): State<DbPool>,
    VPath(tag_path): VPath<TagPath>,
    VQuery(query): VQuery<PostPaginationQuery>,
) -> Result<Json<ApiResponse<Vec<Post>>>, AppError> {
    let client = pool;
    let (offset, limit, search, order_by, order_direction) = post_pagination_params(&query);

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

    Ok(Json(ApiResponse::with_meta_message(
        "Successfully retrieved posts by tag",
        posts,
        total,
        limit,
        offset,
    )))
}

pub async fn get_posts_by_username(
    State(pool): State<DbPool>,
    VPath(params): VPath<UsernamePath>,
    VQuery(query): VQuery<PostPaginationQuery>,
) -> Result<Json<ApiResponse<Vec<Post>>>, AppError> {
    let client = pool;
    let (offset, limit, _, _, _) = post_pagination_params(&query);

    let (posts, total) =
        services::post::get_posts_by_username(&client, &params.username, offset, limit).await?;

    Ok(Json(ApiResponse::with_meta_message(
        "Successfully retrieved posts",
        posts,
        total,
        limit,
        offset,
    )))
}

pub async fn get_my_posts(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VQuery(query): VQuery<PostPaginationQuery>,
) -> Result<Json<ApiResponse<Vec<Post>>>, AppError> {
    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(10);
    let (posts, total) =
        services::post::get_posts_by_created_by(&pool, auth_user.id, offset, limit).await?;

    Ok(Json(ApiResponse::with_meta_message(
        "Successfully retrieved posts",
        posts,
        total,
        limit,
        offset,
    )))
}

pub async fn get_my_post(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VPath(params): VPath<PostIdPath>,
) -> Result<Json<ApiResponse<Post>>, AppError> {
    match services::post::get_post_by_id_for_author(&pool, params.id, auth_user.id).await? {
        Some(post) => Ok(Json(ApiResponse::success_with_message(
            "Successfully retrieved post",
            post,
        ))),
        None => Err(AppError::NotFound("Post not found".to_string())),
    }
}

pub async fn update_my_post(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VPath(params): VPath<PostIdPath>,
    VJson(req): VJson<UpdatePostRequest>,
) -> Result<Json<ApiResponse<Post>>, AppError> {
    ensure_author(&pool, params.id, &auth_user).await?;
    match services::post::update_post(
        &pool,
        params.id,
        services::post::UpdatePostInput {
            title: req.title,
            photo_url: req.photo_url,
            slug: req.slug,
            body: req.body,
            published: req.published,
            tags: req.tags,
        },
    )
    .await?
    {
        Some(post) => Ok(Json(ApiResponse::success_with_message(
            "Post updated successfully",
            post,
        ))),
        None => Err(AppError::NotFound("Post not found".to_string())),
    }
}

pub async fn delete_my_post(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VPath(params): VPath<PostIdPath>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    ensure_author(&pool, params.id, &auth_user).await?;
    match services::post::soft_delete_post(&pool, params.id).await? {
        true => Ok(Json(ApiResponse::success_with_message(
            "Successfully deleted post",
            serde_json::Value::Null,
        ))),
        false => Err(AppError::NotFound("Post not found".to_string())),
    }
}

pub async fn get_post(
    State(pool): State<DbPool>,
    _admin_user: AdminUser,
    VPath(params): VPath<PostIdPath>,
) -> Result<Json<ApiResponse<Post>>, AppError> {
    let client = pool;
    match services::post::get_post_by_id(&client, params.id).await {
        Ok(Some(post)) => Ok(Json(ApiResponse::success_with_message(
            "Successfully retrieved post",
            post,
        ))),
        Ok(None) => Err(AppError::NotFound(format!("Post not found: {}", params.id))),
        Err(e) => Err(AppError::from(e)),
    }
}

pub async fn update_post(
    State(pool): State<DbPool>,
    _admin_user: AdminUser,
    VPath(params): VPath<PostIdPath>,
    VJson(req): VJson<UpdatePostRequest>,
) -> Result<Json<ApiResponse<Post>>, AppError> {
    match services::post::update_post(
        &pool,
        params.id,
        services::post::UpdatePostInput {
            title: req.title,
            photo_url: req.photo_url,
            slug: req.slug,
            body: req.body,
            published: req.published,
            tags: req.tags,
        },
    )
    .await?
    {
        Some(post) => Ok(Json(ApiResponse::success_with_message(
            "Post updated successfully",
            post,
        ))),
        None => Err(AppError::NotFound("Post not found".to_string())),
    }
}

pub async fn delete_post(
    State(pool): State<DbPool>,
    _admin_user: AdminUser,
    VPath(params): VPath<PostIdPath>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    match services::post::soft_delete_post(&pool, params.id).await? {
        true => Ok(Json(ApiResponse::success_with_message(
            "Successfully deleted post",
            serde_json::Value::Null,
        ))),
        false => Err(AppError::NotFound("Post not found".to_string())),
    }
}

pub async fn get_post_by_username_and_slug(
    State(pool): State<DbPool>,
    VPath(params): VPath<PostPath>,
) -> Result<Json<ApiResponse<Post>>, AppError> {
    let client = pool;
    match services::post::get_post_by_username_and_slug(&client, &params.username, &params.slug)
        .await
    {
        Ok(Some(post)) => Ok(Json(ApiResponse::success_with_message(
            "Successfully retrieved post",
            post,
        ))),
        Ok(None) => Err(AppError::NotFound(format!(
            "Post not found: {} by {}",
            params.slug, params.username
        ))),
        Err(e) => Err(AppError::from(e)),
    }
}
