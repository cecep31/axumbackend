use crate::auth::AuthUser;
use crate::database::DbPool;
use crate::dto::common::{PostIdPath, UsernamePath};
use crate::dto::post::{
    CreatePostRequest, MyPostsAnalyticsQuery, MyPostsLikesByMonthQuery, PostPaginationQuery,
    PostPath, RandomPostQuery, TagPath, UpdatePostRequest, post_pagination_params,
};
use crate::error::AppError;
use crate::models::post::{Post, SitemapPost};
use crate::response::ApiResponse;
use crate::services;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
use axum_valid::Valid;
use uuid::Uuid;

pub async fn create_post(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    Valid(Json(req)): Valid<Json<CreatePostRequest>>,
) -> Result<Json<ApiResponse<Post>>, AppError> {
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

    Ok(Json(ApiResponse::success_with_message(
        "Successfully created post",
        post,
    )))
}

fn map_post_view_error(err: services::post_view::PostViewError) -> AppError {
    match err {
        services::post_view::PostViewError::Db(err) => AppError::from(err),
        services::post_view::PostViewError::PostNotFound => {
            AppError::NotFound("Post not found".to_string())
        }
    }
}

fn map_post_like_error(err: services::post_like::PostLikeError) -> AppError {
    match err {
        services::post_like::PostLikeError::Db(err) => AppError::from(err),
        services::post_like::PostLikeError::PostNotFound => {
            AppError::NotFound("Post not found".to_string())
        }
        services::post_like::PostLikeError::AlreadyLiked => {
            AppError::BadRequest("You have already liked this post".to_string())
        }
        services::post_like::PostLikeError::NotLiked => {
            AppError::BadRequest("You have not liked this post".to_string())
        }
    }
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

pub async fn get_posts(
    State(pool): State<DbPool>,
    Valid(query): Valid<Query<PostPaginationQuery>>,
) -> Result<Json<ApiResponse<Vec<Post>>>, AppError> {
    let client = pool;
    let (offset, limit, search, order_by, order_direction) = post_pagination_params(&query);

    let (posts, total) =
        services::post::get_all_posts(&client, offset, limit, search, order_by, order_direction)
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
    Valid(query): Valid<Query<RandomPostQuery>>,
) -> Result<Json<ApiResponse<Vec<Post>>>, AppError> {
    let client = pool;
    let limit = query.limit.unwrap_or(6);
    let limit = limit.min(20);
    let posts = services::post::get_random_posts(&client, limit).await?;
    Ok(Json(ApiResponse::success_with_message(
        "Successfully retrieved posts",
        posts,
    )))
}

pub async fn get_trending_posts(
    State(pool): State<DbPool>,
    Valid(query): Valid<Query<RandomPostQuery>>,
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
    _auth_user: AuthUser,
    Valid(query): Valid<Query<PostPaginationQuery>>,
) -> Result<Json<ApiResponse<Vec<Post>>>, AppError> {
    let (offset, limit, search, order_by, order_direction) = post_pagination_params(&query);
    let (posts, total) =
        services::post::get_all_posts(&pool, offset, limit, search, order_by, order_direction)
            .await?;

    Ok(Json(ApiResponse::with_meta_message(
        "Successfully retrieved posts",
        posts,
        total,
        limit,
        offset,
    )))
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
    Valid(Path(tag_path)): Valid<Path<TagPath>>,
    Valid(query): Valid<Query<PostPaginationQuery>>,
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

fn header_string(headers: &axum::http::HeaderMap, name: &'static str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned)
}

pub async fn record_view(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    headers: axum::http::HeaderMap,
    Valid(Path(params)): Valid<Path<PostIdPath>>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    let ip_address =
        header_string(&headers, "x-forwarded-for").or_else(|| header_string(&headers, "x-real-ip"));
    let user_agent = header_string(&headers, "user-agent");

    services::post_view::record_view(&pool, params.id, Some(auth_user.id), ip_address, user_agent)
        .await
        .map_err(map_post_view_error)?;

    Ok(Json(ApiResponse::success_with_message(
        "View recorded successfully",
        serde_json::Value::Null,
    )))
}

pub async fn get_post_views(
    State(pool): State<DbPool>,
    _auth_user: AuthUser,
    Valid(Path(params)): Valid<Path<PostIdPath>>,
    Valid(query): Valid<Query<PostPaginationQuery>>,
) -> Result<Json<ApiResponse<Vec<crate::models::post_view::PostViewResponse>>>, AppError> {
    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(10);
    let (views, total) = services::post_view::get_views_by_post_id(&pool, params.id, limit, offset)
        .await
        .map_err(map_post_view_error)?;

    Ok(Json(ApiResponse::with_meta_message(
        "Successfully retrieved post views",
        views,
        total,
        limit,
        offset,
    )))
}

pub async fn get_post_view_stats(
    State(pool): State<DbPool>,
    Valid(Path(params)): Valid<Path<PostIdPath>>,
) -> Result<Json<ApiResponse<crate::models::post_view::PostViewStats>>, AppError> {
    let stats = services::post_view::get_view_stats(&pool, params.id)
        .await
        .map_err(map_post_view_error)?;

    Ok(Json(ApiResponse::success_with_message(
        "Successfully retrieved view statistics",
        stats,
    )))
}

pub async fn check_user_viewed(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    Valid(Path(params)): Valid<Path<PostIdPath>>,
) -> Result<Json<ApiResponse<crate::models::post_view::ViewStatusResponse>>, AppError> {
    let status = services::post_view::has_user_viewed_post(&pool, params.id, auth_user.id)
        .await
        .map_err(map_post_view_error)?;

    Ok(Json(ApiResponse::success_with_message(
        "Successfully checked view status",
        status,
    )))
}

pub async fn like_post(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    Valid(Path(params)): Valid<Path<PostIdPath>>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    services::post_like::like_post(&pool, params.id, auth_user.id)
        .await
        .map_err(map_post_like_error)?;

    Ok(Json(ApiResponse::success_with_message(
        "Post liked successfully",
        serde_json::Value::Null,
    )))
}

pub async fn unlike_post(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    Valid(Path(params)): Valid<Path<PostIdPath>>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    services::post_like::unlike_post(&pool, params.id, auth_user.id)
        .await
        .map_err(map_post_like_error)?;

    Ok(Json(ApiResponse::success_with_message(
        "Post unliked successfully",
        serde_json::Value::Null,
    )))
}

pub async fn get_post_likes(
    State(pool): State<DbPool>,
    Valid(Path(params)): Valid<Path<PostIdPath>>,
    Valid(query): Valid<Query<PostPaginationQuery>>,
) -> Result<Json<ApiResponse<crate::models::post_like::PostLikeListResponse>>, AppError> {
    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(10);
    let likes = services::post_like::get_likes_by_post_id(&pool, params.id, limit, offset)
        .await
        .map_err(map_post_like_error)?;

    Ok(Json(ApiResponse::success_with_message(
        "Post likes retrieved successfully",
        likes,
    )))
}

pub async fn get_post_like_stats(
    State(pool): State<DbPool>,
    Valid(Path(params)): Valid<Path<PostIdPath>>,
) -> Result<Json<ApiResponse<crate::models::post_like::PostLikeStats>>, AppError> {
    let stats = services::post_like::get_like_stats(&pool, params.id)
        .await
        .map_err(map_post_like_error)?;

    Ok(Json(ApiResponse::success_with_message(
        "Like stats retrieved successfully",
        stats,
    )))
}

pub async fn check_user_liked(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    Valid(Path(params)): Valid<Path<PostIdPath>>,
) -> Result<Json<ApiResponse<crate::models::post_like::LikeStatusResponse>>, AppError> {
    let status = services::post_like::has_user_liked_post(&pool, params.id, auth_user.id)
        .await
        .map_err(map_post_like_error)?;

    Ok(Json(ApiResponse::success_with_message(
        "Like status retrieved successfully",
        status,
    )))
}

pub async fn get_posts_by_username(
    State(pool): State<DbPool>,
    Valid(Path(params)): Valid<Path<UsernamePath>>,
    Valid(query): Valid<Query<PostPaginationQuery>>,
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
    Valid(query): Valid<Query<PostPaginationQuery>>,
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
    Valid(Path(params)): Valid<Path<PostIdPath>>,
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
    Valid(Path(params)): Valid<Path<PostIdPath>>,
    Valid(Json(req)): Valid<Json<UpdatePostRequest>>,
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
    Valid(Path(params)): Valid<Path<PostIdPath>>,
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

pub async fn get_my_posts_analytics(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    Valid(query): Valid<Query<MyPostsAnalyticsQuery>>,
) -> Result<Json<ApiResponse<crate::models::post_view::MyPostsAnalyticsResponse>>, AppError> {
    let analytics = services::post_view::get_my_posts_analytics(
        &pool,
        auth_user.id,
        query.start_date,
        query.end_date,
    )
    .await?;

    Ok(Json(ApiResponse::success_with_message(
        "Successfully retrieved post analytics",
        analytics,
    )))
}

pub async fn get_my_posts_likes_by_month(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    Valid(query): Valid<Query<MyPostsLikesByMonthQuery>>,
) -> Result<Json<ApiResponse<Vec<crate::models::post_view::MyPostsLikesByMonthResponse>>>, AppError>
{
    let data = services::post_view::get_my_posts_likes_by_month(
        &pool,
        auth_user.id,
        query.months.unwrap_or(12),
    )
    .await?;

    Ok(Json(ApiResponse::success_with_message(
        "Successfully retrieved likes by month",
        data,
    )))
}

pub async fn get_post(
    State(pool): State<DbPool>,
    Valid(Path(params)): Valid<Path<PostIdPath>>,
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
    auth_user: AuthUser,
    Valid(Path(params)): Valid<Path<PostIdPath>>,
    Valid(Json(req)): Valid<Json<UpdatePostRequest>>,
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

pub async fn delete_post(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    Valid(Path(params)): Valid<Path<PostIdPath>>,
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

pub async fn get_post_by_username_and_slug(
    State(pool): State<DbPool>,
    Valid(Path(params)): Valid<Path<PostPath>>,
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

pub fn routes() -> Router<DbPool> {
    Router::new()
        .route("/api/posts", get(get_posts).post(create_post))
        .route("/api/posts/random", get(get_random_posts))
        .route("/api/posts/trending", get(get_trending_posts))
        .route("/api/posts/me", get(get_my_posts))
        .route("/api/posts/me/analytics", get(get_my_posts_analytics))
        .route(
            "/api/posts/me/analytics/likes-by-month",
            get(get_my_posts_likes_by_month),
        )
        .route(
            "/api/posts/me/{id}",
            get(get_my_post).put(update_my_post).delete(delete_my_post),
        )
        .route("/api/posts/feed/for-you", get(get_posts_for_you))
        .route("/api/posts/sitemap", get(get_posts_for_sitemap))
        .route("/api/posts/username/{username}", get(get_posts_by_username))
        .route(
            "/api/posts/u/{username}/{slug}",
            get(get_post_by_username_and_slug),
        )
        .route("/api/posts/tag/{tag}", get(get_posts_by_tag))
        .route("/api/posts/{id}/view", post(record_view))
        .route("/api/posts/{id}/views", get(get_post_views))
        .route("/api/posts/{id}/view-stats", get(get_post_view_stats))
        .route("/api/posts/{id}/viewed", get(check_user_viewed))
        .route("/api/posts/{id}/like", post(like_post).delete(unlike_post))
        .route("/api/posts/{id}/likes", get(get_post_likes))
        .route("/api/posts/{id}/like-stats", get(get_post_like_stats))
        .route("/api/posts/{id}/liked", get(check_user_liked))
        .route(
            "/api/posts/{id}",
            get(get_post).put(update_post).delete(delete_post),
        )
}
