use crate::dto::common::PostIdPath;
use crate::dto::post::{
    CreatePostRequest, PostPaginationQuery, PostPath, PostsFilterQuery, PostsUsernamePath,
    RandomPostQuery, SitemapQuery, TagPath, UpdatePostRequest, parse_go_bool,
    post_pagination_params, sort_direction,
};
use crate::http::auth::{AdminUser, AuthUser};
use crate::http::error::AppError;
use crate::http::extract::{VJson, VPath, VQuery};
use crate::http::response::ApiResponse;
use crate::infra::database::DbPool;
use crate::models::post::{CreatedPostResponse, Post, PostImageUploadResponse, SitemapPost};
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
            services::post::PostWriteError::NotFound => post_not_found(),
            services::post::PostWriteError::NotAuthor => {
                AppError::Forbidden("You are not the post author".to_string())
            }
            services::post::PostWriteError::Db(e) => AppError::from(e),
        }
    }
}

/// echobackend `apperrors.ErrPostNotFound`; handlers add their own message
/// with [`AppError::context`] (`response.NotFound(c, message, err)`).
fn post_not_found() -> AppError {
    AppError::NotFound("post not found".to_string())
}

pub async fn create_post(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VJson(req): VJson<CreatePostRequest>,
) -> Result<
    (
        axum::http::StatusCode,
        Json<ApiResponse<CreatedPostResponse>>,
    ),
    AppError,
> {
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
    .await
    .map_err(|e| AppError::from(e).context("Failed to create post"))?;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(ApiResponse::success_with_message(
            "Successfully created post",
            CreatedPostResponse { id: post.id },
        )),
    ))
}

/// echobackend `IsAuthor`: 404 when the post is missing (or its author is
/// soft-deleted), 403 when someone else wrote it.
async fn ensure_author(pool: &DbPool, post_id: Uuid, auth_user: &AuthUser) -> Result<(), AppError> {
    match services::post::is_author(pool, post_id, auth_user.id).await? {
        Some(true) => Ok(()),
        Some(false) => Err(AppError::Forbidden(
            "You are not the post author".to_string(),
        )),
        None => Err(post_not_found()),
    }
}

/// Parses a `start_date`/`end_date` value. echobackend hands the raw string to
/// Postgres, so both a bare `YYYY-MM-DD` (midnight UTC) and full timestamps
/// (RFC 3339, or `YYYY-MM-DD[T ]HH:MM:SS` read as UTC) are accepted; anything
/// else is a `400` rather than echobackend's database error.
fn parse_date_param(
    raw: Option<&str>,
    field: &str,
) -> Result<Option<chrono::DateTime<chrono::Utc>>, AppError> {
    let Some(value) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(value) {
        return Ok(Some(dt.with_timezone(&chrono::Utc)));
    }
    for format in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f"] {
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(value, format) {
            return Ok(Some(naive.and_utc()));
        }
    }
    chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map(|date| Some(date.and_time(chrono::NaiveTime::MIN).and_utc()))
        .map_err(|_| {
            AppError::BadRequest(format!(
                "Invalid {field} format, expected YYYY-MM-DD or an RFC 3339 timestamp"
            ))
        })
}

pub async fn get_posts(
    State(pool): State<DbPool>,
    VQuery(query): VQuery<PostsFilterQuery>,
) -> Result<Json<ApiResponse<Vec<Post>>>, AppError> {
    let (limit, offset) = query.resolve();

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
    let sort_order = sort_direction(query.sort_order.as_deref());

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
            tags,
        },
    )
    .await
    .map_err(|e| AppError::from(e).context("Failed to get posts"))?;

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
    // echobackend: ParsePaginationParams(9), then capped at 20.
    let limit = query.limit_or(9).min(20);
    let posts = services::post::get_random_posts(&client, limit)
        .await
        .map_err(|e| AppError::from(e).context("Failed to get posts"))?;
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
    let limit = query.limit_or(10);
    let posts = services::post::get_trending_posts(&client, limit)
        .await
        .map_err(|e| AppError::from(e).context("Failed to get trending posts"))?;
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
    let (posts, total) = services::post::get_posts_for_you(&pool, auth_user.id, offset, limit)
        .await
        .map_err(|e| AppError::from(e).context("Failed to get posts"))?;

    Ok(Json(ApiResponse::with_meta_message(
        "Successfully retrieved for-you posts",
        posts,
        total,
        limit,
        offset,
    )))
}

/// Post image upload settings: `image` field, 1 MB cap, JPEG/PNG/GIF/WEBP.
const POST_IMAGE_UPLOAD: crate::http::upload::ImageUploadSpec =
    crate::http::upload::ImageUploadSpec {
        field: "image",
        max_file_bytes: MAX_POST_IMAGE_SIZE,
        label: "Image",
        key_prefix: POST_IMAGE_PREFIX,
        log_name: "post image",
    };

pub async fn upload_image_posts(
    _auth_user: AuthUser,
    headers: axum::http::HeaderMap,
    mut multipart: Multipart,
) -> Result<
    (
        axum::http::StatusCode,
        Json<ApiResponse<PostImageUploadResponse>>,
    ),
    AppError,
> {
    let key =
        crate::http::upload::upload_image(&headers, &mut multipart, &POST_IMAGE_UPLOAD).await?;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(ApiResponse::success_with_message(
            "Successfully uploaded image",
            PostImageUploadResponse::from_key(key),
        )),
    ))
}

pub async fn get_posts_for_sitemap(
    State(pool): State<DbPool>,
    VQuery(query): VQuery<SitemapQuery>,
) -> Result<Json<ApiResponse<Vec<SitemapPost>>>, AppError> {
    let posts = services::post::get_posts_for_sitemap(&pool, query.limit())
        .await
        .map_err(|e| AppError::from(e).context("Failed to get posts for sitemap"))?;
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
    // echobackend `GetPostsByTag`: limit/offset plus `sort_by`/`orderBy` and
    // `sort_order`/`orderDirection` (whitelisted, default created_at desc);
    // `search` is ignored.
    let (offset, limit, _search, order_by, order_direction) = post_pagination_params(&query);

    let (posts, total) = services::post::get_posts_by_tag(
        &pool,
        &tag_path.tag,
        offset,
        limit,
        order_by,
        order_direction,
    )
    .await
    .map_err(|e| AppError::from(e).context("Failed to get posts by tag"))?;

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
    VPath(params): VPath<PostsUsernamePath>,
    VQuery(query): VQuery<PostPaginationQuery>,
) -> Result<Json<ApiResponse<Vec<Post>>>, AppError> {
    let (offset, limit, _, _, _) = post_pagination_params(&query);

    let (posts, total) =
        services::post::get_posts_by_username(&pool, &params.username, offset, limit)
            .await
            .map_err(|e| AppError::from(e).context("Failed to get posts"))?;

    Ok(Json(ApiResponse::with_meta_message(
        "Successfully retrieved posts",
        posts,
        total,
        limit,
        offset,
    )))
}

/// echobackend `GetMyPosts`: the caller's posts (drafts and hidden ones
/// included), optionally filtered by `search` (title) and `published`.
pub async fn get_my_posts(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VQuery(query): VQuery<PostPaginationQuery>,
) -> Result<Json<ApiResponse<Vec<Post>>>, AppError> {
    let (limit, offset) = query.resolve(10);
    let published = parse_go_bool(query.published.as_deref());
    let (posts, total) = services::post::get_posts_by_created_by(
        &pool,
        auth_user.id,
        offset,
        limit,
        query.search.as_deref(),
        published,
    )
    .await
    .map_err(|e| AppError::from(e).context("Failed to get posts"))?;

    Ok(Json(ApiResponse::with_meta_message(
        "Successfully retrieved posts",
        posts,
        total,
        limit,
        offset,
    )))
}

/// echobackend `GetMyPost`: `IsAuthor` first (404 missing / 403 someone
/// else's), then the full post, drafts and hidden posts included.
pub async fn get_my_post(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VPath(params): VPath<PostIdPath>,
) -> Result<Json<ApiResponse<Post>>, AppError> {
    ensure_author(&pool, params.id, &auth_user)
        .await
        .map_err(|e| e.context("Failed to check post ownership"))?;
    let post = services::post::get_post_by_id(&pool, params.id)
        .await
        .map_err(AppError::from)
        .and_then(|post| post.ok_or_else(post_not_found))
        .map_err(|e| e.context("Failed to get post"))?;
    Ok(Json(ApiResponse::success_with_message(
        "Successfully retrieved post",
        post,
    )))
}

fn update_input(req: UpdatePostRequest) -> services::post::UpdatePostInput {
    services::post::UpdatePostInput {
        title: req.title,
        photo_url: req.photo_url,
        slug: req.slug,
        body: req.body,
        published: req.published,
        tags: req.tags,
    }
}

/// echobackend `UpdateMyPost`: the anti-spam check runs before any ownership
/// lookup; the write is owner-scoped, so a non-owner gets 404 (403 only for a
/// no-op request on someone else's post).
pub async fn update_my_post(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VPath(params): VPath<PostIdPath>,
    VJson(req): VJson<UpdatePostRequest>,
) -> Result<Json<ApiResponse<Post>>, AppError> {
    let post = services::post::update_post(&pool, params.id, Some(auth_user.id), update_input(req))
        .await
        .map_err(|e| AppError::from(e).context("Failed to update post"))?;
    Ok(Json(ApiResponse::success_with_message(
        "Post updated successfully",
        post,
    )))
}

/// echobackend `DeleteMyPost`: `DELETE .. WHERE id = ? AND created_by = ?`,
/// so a missing post and someone else's post are both 404.
pub async fn delete_my_post(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VPath(params): VPath<PostIdPath>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    delete_post_scoped(&pool, params.id, Some(auth_user.id)).await
}

async fn delete_post_scoped(
    pool: &DbPool,
    post_id: Uuid,
    owner: Option<Uuid>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    match services::post::delete_post(pool, post_id, owner).await {
        Ok(true) => Ok(Json(ApiResponse::message("Successfully deleted post"))),
        Ok(false) => Err(post_not_found().context("Failed to delete post")),
        Err(e) => Err(AppError::from(e).context("Failed to delete post")),
    }
}

pub async fn get_post(
    State(pool): State<DbPool>,
    _admin_user: AdminUser,
    VPath(params): VPath<PostIdPath>,
) -> Result<Json<ApiResponse<Post>>, AppError> {
    let post = services::post::get_post_by_id(&pool, params.id)
        .await
        .map_err(AppError::from)
        .and_then(|post| post.ok_or_else(post_not_found))
        .map_err(|e| e.context("Failed to get post"))?;
    Ok(Json(ApiResponse::success_with_message(
        "Successfully retrieved post",
        post,
    )))
}

/// echobackend `UpdatePost` (admin): any post, spam check first.
pub async fn update_post(
    State(pool): State<DbPool>,
    _admin_user: AdminUser,
    VPath(params): VPath<PostIdPath>,
    VJson(req): VJson<UpdatePostRequest>,
) -> Result<Json<ApiResponse<Post>>, AppError> {
    let post = services::post::update_post(&pool, params.id, None, update_input(req))
        .await
        .map_err(|e| AppError::from(e).context("Failed to update post"))?;
    Ok(Json(ApiResponse::success_with_message(
        "Post updated successfully",
        post,
    )))
}

pub async fn delete_post(
    State(pool): State<DbPool>,
    _admin_user: AdminUser,
    VPath(params): VPath<PostIdPath>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    delete_post_scoped(&pool, params.id, None).await
}

pub async fn get_post_by_username_and_slug(
    State(pool): State<DbPool>,
    VPath(params): VPath<PostPath>,
) -> Result<Json<ApiResponse<Post>>, AppError> {
    let post = services::post::get_post_by_username_and_slug(&pool, &params.username, &params.slug)
        .await
        .map_err(AppError::from)
        .and_then(|post| post.ok_or_else(post_not_found))
        .map_err(|e| e.context("Failed to get post"))?;
    Ok(Json(ApiResponse::success_with_message(
        "Successfully retrieved post",
        post,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_date_param_accepts_dates_and_timestamps() {
        let day = parse_date_param(Some("2024-01-02"), "start_date")
            .unwrap()
            .unwrap();
        assert_eq!(day.to_rfc3339(), "2024-01-02T00:00:00+00:00");

        let ts = parse_date_param(Some("2024-01-02T10:30:00+07:00"), "start_date")
            .unwrap()
            .unwrap();
        assert_eq!(ts.to_rfc3339(), "2024-01-02T03:30:00+00:00");

        let naive = parse_date_param(Some("2024-01-02 10:30:00"), "end_date")
            .unwrap()
            .unwrap();
        assert_eq!(naive.to_rfc3339(), "2024-01-02T10:30:00+00:00");

        assert!(parse_date_param(None, "start_date").unwrap().is_none());
        assert!(
            parse_date_param(Some("  "), "start_date")
                .unwrap()
                .is_none()
        );
        assert!(parse_date_param(Some("yesterday"), "start_date").is_err());
    }
}
