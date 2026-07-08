use crate::auth::{AdminUser, AuthUser};
use crate::database::DbPool;
use crate::dto::common::{PaginationQuery, UsernamePath};
use crate::dto::user::{FollowRequest, UserIdPath};
use crate::error::AppError;
use crate::models::user::UserResponse;
use crate::models::user_follow::{FollowResponse, FollowStats};
use crate::response::ApiResponse;
use crate::services;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{delete, get, post},
};
use axum_valid::Valid;

fn map_follow_error(err: services::user_follow::UserFollowError) -> AppError {
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

pub async fn get_users(
    State(pool): State<DbPool>,
    _admin_user: AdminUser,
    Valid(query): Valid<Query<PaginationQuery>>,
) -> Result<Json<ApiResponse<Vec<UserResponse>>>, AppError> {
    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(10);
    let (users, total) = services::user::get_users(&pool, offset, limit).await?;

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
    _admin_user: AdminUser,
    Valid(Path(params)): Valid<Path<UserIdPath>>,
) -> Result<Json<ApiResponse<UserResponse>>, AppError> {
    match services::user::get_admin_by_id(&pool, params.id).await {
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
    Valid(Path(params)): Valid<Path<UserIdPath>>,
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
    Valid(Path(params)): Valid<Path<UserIdPath>>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    match services::user::restore(&pool, params.id).await {
        Ok(true) => Ok(Json(ApiResponse::success_with_message(
            "Successfully restored user",
            serde_json::Value::Null,
        ))),
        Ok(false) => Err(AppError::NotFound("User not found".to_string())),
        Err(e) => Err(AppError::from(e)),
    }
}

pub async fn get_by_username(
    State(pool): State<DbPool>,
    Valid(Path(params)): Valid<Path<UsernamePath>>,
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
    Valid(Json(req)): Valid<Json<FollowRequest>>,
) -> Result<Json<ApiResponse<FollowResponse>>, AppError> {
    let response = services::user_follow::follow_user(&pool, auth_user.id, req.user_id)
        .await
        .map_err(map_follow_error)?;

    Ok(Json(ApiResponse::success_with_message(
        response.message.clone(),
        response,
    )))
}

pub async fn unfollow_user(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    Valid(Path(params)): Valid<Path<UserIdPath>>,
) -> Result<Json<ApiResponse<FollowResponse>>, AppError> {
    let response = services::user_follow::unfollow_user(&pool, auth_user.id, params.id)
        .await
        .map_err(map_follow_error)?;

    Ok(Json(ApiResponse::success_with_message(
        response.message.clone(),
        response,
    )))
}

pub async fn check_follow_status(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    Valid(Path(params)): Valid<Path<UserIdPath>>,
) -> Result<Json<ApiResponse<FollowResponse>>, AppError> {
    let is_following = services::user_follow::is_following(&pool, auth_user.id, params.id).await?;
    let response = FollowResponse {
        is_following,
        message: if is_following {
            "User is followed".to_string()
        } else {
            "User is not followed".to_string()
        },
    };

    Ok(Json(ApiResponse::success_with_message(
        "Successfully checked follow status",
        response,
    )))
}

pub async fn get_followers(
    State(pool): State<DbPool>,
    Valid(Path(params)): Valid<Path<UserIdPath>>,
    Valid(query): Valid<Query<PaginationQuery>>,
) -> Result<Json<ApiResponse<Vec<UserResponse>>>, AppError> {
    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(10);
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
    Valid(Path(params)): Valid<Path<UserIdPath>>,
    Valid(query): Valid<Query<PaginationQuery>>,
) -> Result<Json<ApiResponse<Vec<UserResponse>>>, AppError> {
    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(10);
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
    Valid(Path(params)): Valid<Path<UserIdPath>>,
) -> Result<Json<ApiResponse<FollowStats>>, AppError> {
    match services::user_follow::get_follow_stats(&pool, params.id).await? {
        Some(stats) => Ok(Json(ApiResponse::success_with_message(
            "Successfully retrieved follow stats",
            stats,
        ))),
        None => Err(AppError::NotFound("User not found".to_string())),
    }
}

pub async fn get_mutual_follows(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    Valid(Path(params)): Valid<Path<UserIdPath>>,
) -> Result<Json<ApiResponse<Vec<UserResponse>>>, AppError> {
    let users = services::user_follow::get_mutual_follows(&pool, auth_user.id, params.id).await?;

    Ok(Json(ApiResponse::success_with_message(
        "Successfully retrieved mutual follows",
        users,
    )))
}

pub fn routes() -> Router<DbPool> {
    Router::new()
        .route("/api/users", get(get_users))
        .route("/api/users/me", get(get_me))
        .route("/api/users/username/{username}", get(get_by_username))
        .route("/api/users/follow", post(follow_user))
        .route("/api/users/{id}/follow", delete(unfollow_user))
        .route("/api/users/{id}/follow-status", get(check_follow_status))
        .route("/api/users/{id}/mutual-follows", get(get_mutual_follows))
        .route("/api/users/{id}/followers", get(get_followers))
        .route("/api/users/{id}/following", get(get_following))
        .route("/api/users/{id}/follow-stats", get(get_follow_stats))
        .route("/api/users/{id}/restore", post(restore_user))
        .route("/api/users/{id}", get(get_by_id).delete(delete_user))
}
