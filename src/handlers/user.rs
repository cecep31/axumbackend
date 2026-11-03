use crate::dto::common::{PaginationQuery, UsernamePath};
use crate::dto::stats::StatsQuery;
use crate::dto::user::{
    CreateUserRequest, FollowRequest, UpdateUserRequest, UserDeletedFilter, UserDetailQuery,
    UserIdPath, UserListQuery,
};
use crate::dto::validation::parse_pagination;
use crate::http::auth::{AdminUser, AuthUser};
use crate::http::error::AppError;
use crate::http::extract::{VJson, VPath, VQuery};
use crate::http::response::ApiResponse;
use crate::infra::database::DbPool;
use crate::models::stats::UserStatsResponse;
use crate::models::user::{AvatarUploadResponse, UserResponse};
use crate::models::user_follow::{FollowResponse, FollowStats, FollowStatusResponse};
use crate::services;
use crate::state::AppState;
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Multipart, State},
    http::StatusCode,
    routing::{delete, get, post},
};

/// Maximum avatar file size. The route body limit adds multipart framing
/// slack on top (see `crate::http::upload::max_body_bytes`) so files exactly at
/// the cap still stream fully and are judged by the handler, not cut off
/// mid-stream by the limiter.
const MAX_AVATAR_SIZE: u64 = 5 * 1024 * 1024;
/// Object key prefix for avatars; matches echobackend.
const AVATAR_PREFIX: &str = "users/avatars";

impl From<services::user::UserError> for AppError {
    fn from(err: services::user::UserError) -> Self {
        match err {
            services::user::UserError::Db(err) => AppError::from(err),
            services::user::UserError::NotFound => AppError::NotFound("user not found".to_string()),
            services::user::UserError::UserExists => {
                AppError::Conflict("Email or username already exists".to_string())
            }
            services::user::UserError::InvalidData(msg) => AppError::BadRequest(msg),
        }
    }
}

/// Carries echobackend's sentinel texts as the `error` detail; handlers add
/// their message with [`AppError::context`] (echobackend `respondError`).
impl From<services::user_follow::UserFollowError> for AppError {
    fn from(err: services::user_follow::UserFollowError) -> Self {
        match err {
            services::user_follow::UserFollowError::Db(err) => AppError::from(err),
            services::user_follow::UserFollowError::UserNotFound => {
                AppError::NotFound("user not found".to_string())
            }
            services::user_follow::UserFollowError::CannotFollowSelf => {
                AppError::BadRequest("cannot follow yourself".to_string())
            }
            services::user_follow::UserFollowError::AlreadyFollowing => {
                AppError::Conflict("already following this user".to_string())
            }
            services::user_follow::UserFollowError::NotFollowing => {
                AppError::BadRequest("not following this user".to_string())
            }
        }
    }
}

/// echobackend `respondError(c, message, err)` for follow/user service errors.
fn follow_error(
    message: &'static str,
) -> impl FnOnce(services::user_follow::UserFollowError) -> AppError {
    move |err| AppError::from(err).context(message)
}

fn db_error(message: &'static str) -> impl FnOnce(sea_orm::DbErr) -> AppError {
    move |err| AppError::from(err).context(message)
}

/// 404 of echobackend's user lookups: `respondError(c, message, ErrUserNotFound)`.
fn user_not_found(message: &'static str) -> AppError {
    AppError::not_found(message, Some("user not found".into()))
}

pub async fn get_users(
    State(pool): State<DbPool>,
    _admin_user: AdminUser,
    VQuery(query): VQuery<UserListQuery>,
) -> Result<Json<ApiResponse<Vec<UserResponse>>>, AppError> {
    let (limit, offset) = parse_pagination(query.offset.as_deref(), query.limit.as_deref(), 10);
    let deleted_filter =
        UserDeletedFilter::parse(query.deleted.as_deref()).map_err(AppError::BadRequest)?;
    // echobackend `ParseUserOrderDesc`: only the exact string "asc" ascends.
    let order_desc = query.order_direction.as_deref() != Some("asc");
    let (users, total) = services::user::get_users(
        &pool,
        offset,
        limit,
        deleted_filter,
        query.order_by.as_deref(),
        order_desc,
    )
    .await
    .map_err(db_error("Failed to retrieve users"))?;

    Ok(Json(ApiResponse::with_meta_message(
        "Successfully retrieved users",
        users,
        total,
        limit,
        offset,
    )))
}

pub async fn get_by_id(
    State(pool): State<DbPool>,
    admin_user: AdminUser,
    VPath(params): VPath<UserIdPath>,
    VQuery(query): VQuery<UserDetailQuery>,
) -> Result<Json<ApiResponse<UserResponse>>, AppError> {
    let result = if query.deleted.as_deref() == Some("true") {
        services::user::get_admin_by_id_deleted_only(&pool, params.id).await
    } else {
        services::user::get_admin_by_id(&pool, params.id, Some(admin_user.0.id)).await
    };

    match result {
        Ok(Some(user)) => Ok(Json(ApiResponse::success_with_message(
            "Successfully retrieved user",
            user,
        ))),
        Ok(None) => Err(user_not_found("Failed to retrieve user")),
        Err(e) => Err(AppError::from(e).context("Failed to retrieve user")),
    }
}

pub async fn get_me(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
) -> Result<Json<ApiResponse<UserResponse>>, AppError> {
    match services::user::get_current_user(&pool, auth_user.id).await {
        Ok(Some(user)) => Ok(Json(ApiResponse::success_with_message(
            "Successfully retrieved current user",
            user,
        ))),
        Ok(None) => Err(user_not_found("Failed to retrieve user")),
        Err(e) => Err(AppError::from(e).context("Failed to retrieve user")),
    }
}

pub async fn delete_user(
    State(pool): State<DbPool>,
    _admin_user: AdminUser,
    VPath(params): VPath<UserIdPath>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    match services::user::soft_delete(&pool, params.id).await {
        Ok(true) => Ok(Json(ApiResponse::message("Successfully deleted user"))),
        Ok(false) => Err(user_not_found("Failed to delete user")),
        Err(e) => Err(AppError::from(e).context("Failed to delete user")),
    }
}

pub async fn restore_user(
    State(pool): State<DbPool>,
    _admin_user: AdminUser,
    VPath(params): VPath<UserIdPath>,
) -> Result<Json<ApiResponse<UserResponse>>, AppError> {
    match services::user::restore(&pool, params.id).await {
        Ok(user) => Ok(Json(ApiResponse::success_with_message(
            "Successfully restored user",
            user,
        ))),
        Err(services::user::RestoreError::NotFound) => {
            Err(user_not_found("Deleted user not found"))
        }
        Err(services::user::RestoreError::Conflict) => Err(AppError::conflict(
            "Cannot restore user",
            "Email or username already taken by another active user",
        )),
        Err(services::user::RestoreError::Db(e)) => {
            Err(AppError::from(e).context("Failed to restore user"))
        }
    }
}

pub async fn get_by_username(
    State(pool): State<DbPool>,
    VPath(params): VPath<UsernamePath>,
) -> Result<Json<ApiResponse<UserResponse>>, AppError> {
    match services::user::get_by_username(&pool, &params.username).await {
        Ok(Some(user)) => Ok(Json(ApiResponse::success_with_message(
            "Successfully retrieved user",
            user,
        ))),
        Ok(None) => Err(user_not_found("Failed to retrieve user")),
        Err(e) => Err(AppError::from(e).context("Failed to retrieve user")),
    }
}

pub async fn follow_user(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VJson(req): VJson<FollowRequest>,
) -> Result<Json<ApiResponse<FollowResponse>>, AppError> {
    let target = req
        .user_uuid()
        .ok_or_else(|| AppError::BadRequest("Invalid request body".to_string()))?;
    let response = services::user_follow::follow_user(&pool, auth_user.id, target)
        .await
        .map_err(follow_error("Failed to follow user"))?;

    Ok(Json(ApiResponse::success_with_message(
        response.message.clone(),
        response,
    )))
}

pub async fn unfollow_user(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VPath(params): VPath<UserIdPath>,
) -> Result<Json<ApiResponse<FollowResponse>>, AppError> {
    let response = services::user_follow::unfollow_user(&pool, auth_user.id, params.id)
        .await
        .map_err(follow_error("Failed to unfollow user"))?;

    Ok(Json(ApiResponse::success_with_message(
        response.message.clone(),
        response,
    )))
}

pub async fn check_follow_status(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VPath(params): VPath<UserIdPath>,
) -> Result<Json<ApiResponse<FollowStatusResponse>>, AppError> {
    let is_following = services::user_follow::is_following(&pool, auth_user.id, params.id)
        .await
        .map_err(db_error("Failed to check follow status"))?;

    Ok(Json(ApiResponse::success_with_message(
        "Successfully checked follow status",
        FollowStatusResponse { is_following },
    )))
}

pub async fn get_followers(
    State(pool): State<DbPool>,
    VPath(params): VPath<UserIdPath>,
    VQuery(query): VQuery<PaginationQuery>,
) -> Result<Json<ApiResponse<Vec<UserResponse>>>, AppError> {
    let (limit, offset) = query.resolve(10);
    let (followers, total) =
        services::user_follow::get_followers(&pool, params.id, limit, offset, None)
            .await
            .map_err(db_error("Failed to get followers"))?;

    Ok(Json(ApiResponse::with_meta_message(
        "Successfully retrieved followers",
        followers,
        total,
        limit,
        offset,
    )))
}

pub async fn get_following(
    State(pool): State<DbPool>,
    VPath(params): VPath<UserIdPath>,
    VQuery(query): VQuery<PaginationQuery>,
) -> Result<Json<ApiResponse<Vec<UserResponse>>>, AppError> {
    let (limit, offset) = query.resolve(10);
    let (following, total) =
        services::user_follow::get_following(&pool, params.id, limit, offset, None)
            .await
            .map_err(db_error("Failed to get following"))?;

    Ok(Json(ApiResponse::with_meta_message(
        "Successfully retrieved following",
        following,
        total,
        limit,
        offset,
    )))
}

pub async fn get_follow_stats(
    State(pool): State<DbPool>,
    VPath(params): VPath<UserIdPath>,
) -> Result<Json<ApiResponse<FollowStats>>, AppError> {
    match services::user_follow::get_follow_stats(&pool, params.id)
        .await
        .map_err(db_error("Failed to get follow statistics"))?
    {
        Some(stats) => Ok(Json(ApiResponse::success_with_message(
            "Successfully retrieved follow statistics",
            stats,
        ))),
        None => Err(user_not_found("Failed to get follow statistics")),
    }
}

pub async fn get_mutual_follows(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VPath(params): VPath<UserIdPath>,
) -> Result<Json<ApiResponse<Vec<UserResponse>>>, AppError> {
    let users = services::user_follow::get_mutual_follows(&pool, auth_user.id, params.id)
        .await
        .map_err(db_error("Failed to get mutual follows"))?;

    Ok(Json(ApiResponse::success_with_message(
        "Successfully retrieved mutual follows",
        users,
    )))
}

/// Avatar upload settings: `avatar` field, 5 MB cap, JPEG/PNG/GIF/WEBP.
const AVATAR_UPLOAD: crate::http::upload::ImageUploadSpec = crate::http::upload::ImageUploadSpec {
    field: "avatar",
    max_file_bytes: MAX_AVATAR_SIZE,
    label: "Avatar",
    key_prefix: AVATAR_PREFIX,
    log_name: "avatar",
};

pub async fn upload_avatar(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    headers: axum::http::HeaderMap,
    mut multipart: Multipart,
) -> Result<Json<ApiResponse<AvatarUploadResponse>>, AppError> {
    let key = crate::http::upload::upload_image(&headers, &mut multipart, &AVATAR_UPLOAD).await?;
    services::user::set_image(&pool, auth_user.id, &key).await?;

    Ok(Json(ApiResponse::success_with_message(
        "Profile picture updated successfully",
        AvatarUploadResponse { image: key },
    )))
}

pub async fn get_user_stats(
    State(pool): State<DbPool>,
    _admin_user: AdminUser,
    VQuery(query): VQuery<StatsQuery>,
) -> Result<Json<ApiResponse<UserStatsResponse>>, AppError> {
    let stats = services::stats::user_stats(&pool, query.date_range(), query.limit()).await?;
    Ok(Json(ApiResponse::success_with_message(
        "User stats fetched successfully",
        stats,
    )))
}

pub async fn create_user(
    State(pool): State<DbPool>,
    _admin_user: AdminUser,
    VJson(req): VJson<CreateUserRequest>,
) -> Result<(StatusCode, Json<ApiResponse<UserResponse>>), AppError> {
    let user = services::user::create_user(&pool, req)
        .await
        .map_err(|err| match err {
            services::user::UserError::UserExists => {
                AppError::conflict("Failed to create user", "Email or username already exists")
            }
            other => AppError::from(other).context("Failed to create user"),
        })?;

    Ok((
        StatusCode::CREATED,
        Json(ApiResponse::success_with_message(
            "User created successfully",
            user,
        )),
    ))
}

pub async fn update_user(
    State(pool): State<DbPool>,
    _admin_user: AdminUser,
    VPath(params): VPath<UserIdPath>,
    VJson(req): VJson<UpdateUserRequest>,
) -> Result<Json<ApiResponse<UserResponse>>, AppError> {
    let user = services::user::update_user(&pool, params.id, req)
        .await
        .map_err(|err| match err {
            services::user::UserError::UserExists => {
                AppError::conflict("Failed to update user", "Email or username already taken")
            }
            other => AppError::from(other).context("Failed to update user"),
        })?;

    Ok(Json(ApiResponse::success_with_message(
        "User updated successfully",
        user,
    )))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/users", get(get_users).post(create_user))
        .route("/api/users/stats", get(get_user_stats))
        .route("/api/users/me", get(get_me))
        .route(
            "/api/users/me/image",
            post(upload_avatar).route_layer(DefaultBodyLimit::max(
                crate::http::upload::max_body_bytes(MAX_AVATAR_SIZE),
            )),
        )
        .route("/api/users/username/{username}", get(get_by_username))
        .route("/api/users/follow", post(follow_user))
        .route("/api/users/{id}/follow", delete(unfollow_user))
        .route("/api/users/{id}/follow-status", get(check_follow_status))
        .route("/api/users/{id}/mutual-follows", get(get_mutual_follows))
        .route("/api/users/{id}/followers", get(get_followers))
        .route("/api/users/{id}/following", get(get_following))
        .route("/api/users/{id}/follow-stats", get(get_follow_stats))
        .route("/api/users/{id}/restore", post(restore_user))
        .route(
            "/api/users/{id}",
            get(get_by_id).put(update_user).delete(delete_user),
        )
}
