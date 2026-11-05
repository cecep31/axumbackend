use crate::dto::common::{PaginationQuery, UsernamePath};
use crate::dto::stats::StatsQuery;
use crate::dto::user::{
    CreateUserRequest, FollowRequest, UpdateUserRequest, UserDeletedFilter, UserDetailQuery,
    UserIdPath, UserListQuery,
};
use crate::dto::validation::{parse_canonical_uuid, parse_pagination};
use crate::http::auth::{AdminUser, AuthUser};
use crate::http::error::AppError;
use crate::http::extract::{VBind, VPath, VQuery, bind_message};
use crate::http::response::ApiResponse;
use crate::infra::database::DbPool;
use crate::models::stats::UserStatsResponse;
use crate::models::user::{AvatarUploadResponse, UserResponse};
use crate::models::user_follow::{FollowResponse, FollowStats, FollowStatusResponse};
use crate::services;
use crate::state::AppState;
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, FromRequestParts, Multipart, Path, State},
    http::{StatusCode, request::Parts},
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

/// The `{id}` of `/api/users/{id}/...`, checked like echobackend's
/// `validator.IsValidUUID` + `respondInvalidUserID`: anything but a canonical
/// lowercase hyphenated UUID is 400 "Invalid user ID" / "invalid user ID
/// format". Declare it after the `AuthUser` / `AdminUser` argument (auth runs
/// first) and before any body extractor (the id is checked before `c.Bind`).
pub struct UserIdParam(pub uuid::Uuid);

impl<S: Send + Sync> FromRequestParts<S> for UserIdParam {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        Path::<UserIdPath>::from_request_parts(parts, state)
            .await
            .ok()
            .and_then(|Path(path)| parse_canonical_uuid(&path.id))
            .map(UserIdParam)
            .ok_or_else(invalid_user_id)
    }
}

/// echobackend `respondInvalidUserID`.
fn invalid_user_id() -> AppError {
    AppError::bad_request("Invalid user ID", Some("invalid user ID format".into()))
}

pub async fn get_users(
    State(pool): State<DbPool>,
    _admin_user: AdminUser,
    VQuery(query): VQuery<UserListQuery>,
) -> Result<Json<ApiResponse<Vec<UserResponse>>>, AppError> {
    let (limit, offset) = parse_pagination(query.offset.as_deref(), query.limit.as_deref(), 10);
    let deleted_filter = UserDeletedFilter::parse(query.deleted.as_deref())
        .map_err(|err| AppError::bad_request("Invalid deleted filter", Some(err)))?;
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
    UserIdParam(user_id): UserIdParam,
    VQuery(query): VQuery<UserDetailQuery>,
) -> Result<Json<ApiResponse<UserResponse>>, AppError> {
    let result = if query.deleted.as_deref() == Some("true") {
        services::user::get_admin_by_id_deleted_only(&pool, user_id).await
    } else {
        services::user::get_admin_by_id(&pool, user_id, Some(admin_user.0.id)).await
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
    UserIdParam(user_id): UserIdParam,
) -> Result<Json<ApiResponse<()>>, AppError> {
    match services::user::soft_delete(&pool, user_id).await {
        Ok(true) => Ok(Json(ApiResponse::message("Successfully deleted user"))),
        Ok(false) => Err(user_not_found("Failed to delete user")),
        Err(e) => Err(AppError::from(e).context("Failed to delete user")),
    }
}

pub async fn restore_user(
    State(pool): State<DbPool>,
    _admin_user: AdminUser,
    UserIdParam(user_id): UserIdParam,
) -> Result<Json<ApiResponse<UserResponse>>, AppError> {
    match services::user::restore(&pool, user_id).await {
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
    body: Result<VBind<FollowRequest>, AppError>,
) -> Result<Json<ApiResponse<FollowResponse>>, AppError> {
    // echobackend `c.Bind` + `c.Validate`: an empty body is a 422 `required`
    // `UserID`; one that does not bind is a 400 "Invalid request body".
    let VBind(req) = body.map_err(bind_message("Invalid request body"))?;
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
    UserIdParam(user_id): UserIdParam,
) -> Result<Json<ApiResponse<FollowResponse>>, AppError> {
    let response = services::user_follow::unfollow_user(&pool, auth_user.id, user_id)
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
    UserIdParam(user_id): UserIdParam,
) -> Result<Json<ApiResponse<FollowStatusResponse>>, AppError> {
    let is_following = services::user_follow::is_following(&pool, auth_user.id, user_id)
        .await
        .map_err(db_error("Failed to check follow status"))?;

    Ok(Json(ApiResponse::success_with_message(
        "Successfully checked follow status",
        FollowStatusResponse { is_following },
    )))
}

pub async fn get_followers(
    State(pool): State<DbPool>,
    UserIdParam(user_id): UserIdParam,
    VQuery(query): VQuery<PaginationQuery>,
) -> Result<Json<ApiResponse<Vec<UserResponse>>>, AppError> {
    let (limit, offset) = query.resolve(10);
    let (followers, total) =
        services::user_follow::get_followers(&pool, user_id, limit, offset, None)
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
    UserIdParam(user_id): UserIdParam,
    VQuery(query): VQuery<PaginationQuery>,
) -> Result<Json<ApiResponse<Vec<UserResponse>>>, AppError> {
    let (limit, offset) = query.resolve(10);
    let (following, total) =
        services::user_follow::get_following(&pool, user_id, limit, offset, None)
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
    UserIdParam(user_id): UserIdParam,
) -> Result<Json<ApiResponse<FollowStats>>, AppError> {
    match services::user_follow::get_follow_stats(&pool, user_id)
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
    UserIdParam(user_id): UserIdParam,
) -> Result<Json<ApiResponse<Vec<UserResponse>>>, AppError> {
    let users = services::user_follow::get_mutual_follows(&pool, auth_user.id, user_id)
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
    let stats = services::stats::user_stats(&pool, query.date_range(), query.limit())
        .await
        .map_err(db_error("Failed to fetch user stats"))?;
    Ok(Json(ApiResponse::success_with_message(
        "User stats fetched successfully",
        stats,
    )))
}

pub async fn create_user(
    State(pool): State<DbPool>,
    _admin_user: AdminUser,
    VBind(req): VBind<CreateUserRequest>,
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
    UserIdParam(user_id): UserIdParam,
    VBind(req): VBind<UpdateUserRequest>,
) -> Result<Json<ApiResponse<UserResponse>>, AppError> {
    let user = services::user::update_user(&pool, user_id, req)
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

#[cfg(test)]
mod tests {
    use super::*;

    async fn user_id_param(uri: &str) -> Result<uuid::Uuid, AppError> {
        let app = Router::new().route(
            "/api/users/{id}",
            get(|UserIdParam(id): UserIdParam| async move { id.to_string() }),
        );
        let response = tower::ServiceExt::oneshot(
            app,
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
            let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert_eq!(
                body,
                serde_json::json!({"success": false, "message": "Invalid user ID", "error": "invalid user ID format"})
            );
            Err(invalid_user_id())
        }
    }

    async fn follow_body(body: &'static str) -> (StatusCode, serde_json::Value) {
        let app = Router::new().route(
            "/api/users/follow",
            axum::routing::post(|body: Result<VBind<FollowRequest>, AppError>| async move {
                body.map_err(bind_message("Invalid request body"))
                    .map(|VBind(req)| req.user_id)
            }),
        );
        let response = tower::ServiceExt::oneshot(
            app,
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/users/follow")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )
    }

    /// echobackend `FollowUser`: `c.Bind` skips an empty body, so it is a
    /// 422 `required`; an unparsable one is 400 "Invalid request body".
    #[tokio::test]
    async fn follow_body_matches_echobackend_bind() {
        let (status, body) = follow_body("").await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["errors"][0]["field"], "UserID");
        assert_eq!(body["errors"][0]["tag"], "required");

        let (status, body) = follow_body("{").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["message"], "Invalid request body");
    }

    #[tokio::test]
    async fn user_id_param_accepts_only_canonical_uuids() {
        let ok = "0190a8f2-1c3e-7b4a-9d6e-2f1a3b4c5d6e";
        assert_eq!(
            user_id_param(&format!("/api/users/{ok}"))
                .await
                .unwrap()
                .to_string(),
            ok
        );
        for bad in [
            "not-a-uuid",
            "0190A8F2-1C3E-7B4A-9D6E-2F1A3B4C5D6E",
            "0190a8f21c3e7b4a9d6e2f1a3b4c5d6e",
            "%7B0190a8f2-1c3e-7b4a-9d6e-2f1a3b4c5d6e%7D",
        ] {
            assert!(user_id_param(&format!("/api/users/{bad}")).await.is_err());
        }
    }
}
