use crate::auth::{AdminUser, AuthUser};
use crate::database::DbPool;
use crate::dto::common::{PaginationQuery, UsernamePath};
use crate::dto::user::{
    CreateUserRequest, FollowRequest, UpdateUserRequest, UserDeletedFilter, UserDetailQuery,
    UserIdPath, UserListQuery,
};
use crate::error::AppError;
use crate::extract::{VJson, VPath, VQuery};
use crate::models::user::UserResponse;
use crate::models::user_follow::{FollowResponse, FollowStats};
use crate::response::ApiResponse;
use crate::services;
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Multipart, State},
    http::StatusCode,
    routing::{delete, get, post},
};

impl From<services::user::UserError> for AppError {
    fn from(err: services::user::UserError) -> Self {
        match err {
            services::user::UserError::Db(err) => AppError::from(err),
            services::user::UserError::NotFound => AppError::NotFound("User not found".to_string()),
            services::user::UserError::UserExists => {
                AppError::Conflict("Email or username already exists".to_string())
            }
            services::user::UserError::InvalidData(msg) => AppError::BadRequest(msg),
        }
    }
}

impl From<services::user_follow::UserFollowError> for AppError {
    fn from(err: services::user_follow::UserFollowError) -> Self {
        match err {
            services::user_follow::UserFollowError::Db(err) => AppError::from(err),
            services::user_follow::UserFollowError::UserNotFound => {
                AppError::NotFound("User not found".to_string())
            }
            services::user_follow::UserFollowError::CannotFollowSelf => {
                AppError::BadRequest("You cannot follow yourself".to_string())
            }
            services::user_follow::UserFollowError::AlreadyFollowing => {
                AppError::BadRequest("You are already following this user".to_string())
            }
            services::user_follow::UserFollowError::NotFollowing => {
                AppError::BadRequest("You are not following this user".to_string())
            }
        }
    }
}

pub async fn get_users(
    State(pool): State<DbPool>,
    _admin_user: AdminUser,
    VQuery(query): VQuery<UserListQuery>,
) -> Result<Json<ApiResponse<Vec<UserResponse>>>, AppError> {
    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(10);
    let deleted_filter =
        UserDeletedFilter::parse(query.deleted.as_deref()).map_err(AppError::BadRequest)?;
    let (users, total) = services::user::get_users(&pool, offset, limit, deleted_filter).await?;

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
        Ok(None) => Err(AppError::NotFound("User not found".to_string())),
        Err(e) => Err(AppError::from(e)),
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
        Ok(None) => Err(AppError::NotFound("User not found".to_string())),
        Err(e) => Err(AppError::from(e)),
    }
}

pub async fn delete_user(
    State(pool): State<DbPool>,
    _admin_user: AdminUser,
    VPath(params): VPath<UserIdPath>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    match services::user::soft_delete(&pool, params.id).await {
        Ok(true) => Ok(Json(ApiResponse::success_with_message(
            "Successfully deleted user",
            serde_json::Value::Null,
        ))),
        Ok(false) => Err(AppError::NotFound("User not found".to_string())),
        Err(e) => Err(AppError::from(e)),
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
            Err(AppError::NotFound("User not found".to_string()))
        }
        Err(services::user::RestoreError::Conflict) => Err(AppError::Conflict(
            "Cannot restore user: email or username already taken by another active user"
                .to_string(),
        )),
        Err(services::user::RestoreError::Db(e)) => Err(AppError::from(e)),
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
        Ok(None) => Err(AppError::NotFound("User not found".to_string())),
        Err(e) => Err(AppError::from(e)),
    }
}

pub async fn follow_user(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VJson(req): VJson<FollowRequest>,
) -> Result<Json<ApiResponse<FollowResponse>>, AppError> {
    let response = services::user_follow::follow_user(&pool, auth_user.id, req.user_id).await?;

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
    let response = services::user_follow::unfollow_user(&pool, auth_user.id, params.id).await?;

    Ok(Json(ApiResponse::success_with_message(
        response.message.clone(),
        response,
    )))
}

pub async fn check_follow_status(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VPath(params): VPath<UserIdPath>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    let is_following = services::user_follow::is_following(&pool, auth_user.id, params.id).await?;

    Ok(Json(ApiResponse::success_with_message(
        "Successfully checked follow status",
        serde_json::json!({ "is_following": is_following }),
    )))
}

pub async fn get_followers(
    State(pool): State<DbPool>,
    VPath(params): VPath<UserIdPath>,
    VQuery(query): VQuery<PaginationQuery>,
) -> Result<Json<ApiResponse<Vec<UserResponse>>>, AppError> {
    let (limit, offset) = query.resolve(10);
    let (followers, total) =
        services::user_follow::get_followers(&pool, params.id, limit, offset, None).await?;

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
        services::user_follow::get_following(&pool, params.id, limit, offset, None).await?;

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
    match services::user_follow::get_follow_stats(&pool, params.id).await? {
        Some(stats) => Ok(Json(ApiResponse::success_with_message(
            "Successfully retrieved follow statistics",
            stats,
        ))),
        None => Err(AppError::NotFound("User not found".to_string())),
    }
}

pub async fn get_mutual_follows(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VPath(params): VPath<UserIdPath>,
) -> Result<Json<ApiResponse<Vec<UserResponse>>>, AppError> {
    let users = services::user_follow::get_mutual_follows(&pool, auth_user.id, params.id).await?;

    Ok(Json(ApiResponse::success_with_message(
        "Successfully retrieved mutual follows",
        users,
    )))
}

fn detect_allowed_avatar(data: &[u8]) -> bool {
    let is_jpeg = data.len() >= 3 && data[0] == 0xFF && data[1] == 0xD8 && data[2] == 0xFF;
    let is_png = data.len() >= 8 && &data[0..8] == b"\x89PNG\r\n\x1a\n";
    let is_gif = data.len() >= 6 && (&data[0..6] == b"GIF87a" || &data[0..6] == b"GIF89a");
    let is_webp = data.len() >= 12 && &data[0..4] == b"RIFF" && &data[8..12] == b"WEBP";
    is_jpeg || is_png || is_gif || is_webp
}

pub async fn upload_avatar(
    _auth_user: AuthUser,
    mut multipart: Multipart,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    const MAX_AVATAR_SIZE: usize = 5 * 1024 * 1024;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|err| AppError::BadRequest(format!("Failed to upload image: {err}")))?
    {
        if field.name() != Some("avatar") {
            continue;
        }

        let data = field
            .bytes()
            .await
            .map_err(|err| AppError::BadRequest(format!("Failed to read image: {err}")))?;
        if data.len() > MAX_AVATAR_SIZE {
            return Err(AppError::BadRequest("File is too large".to_string()));
        }
        if !detect_allowed_avatar(&data) {
            return Err(AppError::BadRequest("Invalid file type".to_string()));
        }

        return Err(AppError::BadRequest(
            "Storage is not configured".to_string(),
        ));
    }

    Err(AppError::BadRequest("No file uploaded".to_string()))
}

pub async fn create_user(
    State(pool): State<DbPool>,
    _admin_user: AdminUser,
    VJson(req): VJson<CreateUserRequest>,
) -> Result<(StatusCode, Json<ApiResponse<UserResponse>>), AppError> {
    let user = services::user::create_user(&pool, req).await?;

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
                AppError::Conflict("Email or username already taken".to_string())
            }
            other => AppError::from(other),
        })?;

    Ok(Json(ApiResponse::success_with_message(
        "User updated successfully",
        user,
    )))
}

pub fn routes() -> Router<DbPool> {
    Router::new()
        .route("/api/users", get(get_users).post(create_user))
        .route("/api/users/me", get(get_me))
        .route(
            "/api/users/me/image",
            post(upload_avatar).route_layer(DefaultBodyLimit::max(5 * 1024 * 1024)),
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
