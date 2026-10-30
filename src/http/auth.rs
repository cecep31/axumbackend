use crate::config::JwtConfig;
use crate::entities::users;
use crate::http::response::ApiResponse;
use crate::infra::database::DbPool;
use axum::{
    Json,
    extract::{FromRef, FromRequestParts, Request},
    http::{StatusCode, request::Parts},
    middleware::Next,
    response::{IntoResponse, Response},
};
use jsonwebtoken::{DecodingKey, Validation, decode};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QuerySelect};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Decoded access-token claims. Tokens are minted with only `user_id`, `iat`
/// and `exp` (echobackend `createAccessToken`); the legacy `username`,
/// `email` and `is_super_admin` claims of older axum tokens are still
/// accepted but never read. `user_id` is a string, as in echobackend's
/// `jwt.MapClaims`: a token without one is rejected with "Authentication
/// required".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    #[serde(default)]
    pub user_id: String,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub is_super_admin: Option<bool>,
    #[serde(default)]
    pub iat: usize,
    pub exp: usize,
}

/// The caller of an authenticated route: only the `user_id` claim is
/// trusted. `is_super_admin` is `false` unless [`AdminUser`] confirmed it
/// against the database; for an in-handler check use [`is_super_admin`].
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub id: Uuid,
    pub is_super_admin: bool,
}

/// Builds the auth-middleware error envelope documented in the API reference:
/// 401 responses use `error: "Unauthorized access"`, 403 responses use
/// `error: "Access forbidden"`, with a more specific human-readable `message`.
fn auth_error(status: StatusCode, message: impl Into<String>) -> Response {
    let error = if status == StatusCode::FORBIDDEN {
        "Access forbidden"
    } else {
        "Unauthorized access"
    };
    (status, Json(ApiResponse::error(message, error))).into_response()
}

impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        // Already authenticated by the `require_auth` middleware.
        if let Some(user) = parts.extensions.get::<AuthUser>() {
            return Ok(user.clone());
        }

        let Some(header) = parts.headers.get(axum::http::header::AUTHORIZATION) else {
            return Err(auth_error(
                StatusCode::UNAUTHORIZED,
                "Missing authorization header",
            ));
        };

        let Ok(header) = header.to_str() else {
            return Err(auth_error(
                StatusCode::UNAUTHORIZED,
                "Invalid authorization header",
            ));
        };

        let Some(token) = header.strip_prefix("Bearer ") else {
            return Err(auth_error(
                StatusCode::UNAUTHORIZED,
                "Invalid authorization header",
            ));
        };

        let token = decode::<Claims>(
            token,
            &DecodingKey::from_secret(JwtConfig::get().secret.as_bytes()),
            &Validation::default(),
        )
        .map_err(|_| auth_error(StatusCode::UNAUTHORIZED, "Invalid or expired token"))?;

        let Ok(id) = Uuid::parse_str(&token.claims.user_id) else {
            return Err(auth_error(
                StatusCode::UNAUTHORIZED,
                "Authentication required",
            ));
        };

        // The is_super_admin claim is ignored: privileges come from the DB.
        Ok(AuthUser {
            id,
            is_super_admin: false,
        })
    }
}

/// echobackend's `Auth()` as a route middleware, for routes where auth must
/// run *before* another layer (e.g. the per-route rate limiters on `POST
/// /api/posts` and `POST /api/posts/{id}/reports`, so a 401 never consumes
/// the budget). Rejects exactly like the [`AuthUser`] extractor and stores the
/// user in the request extensions, where the extractor picks it up.
pub async fn require_auth(request: Request, next: Next) -> Response {
    let (mut parts, body) = request.into_parts();
    match AuthUser::from_request_parts(&mut parts, &()).await {
        Ok(user) => {
            parts.extensions.insert(user);
            next.run(Request::from_parts(parts, body)).await
        }
        Err(rejection) => rejection,
    }
}

/// Mirrors echobackend's `OptionalAuth` middleware: a valid Bearer token yields
/// the user, while a missing, malformed or expired one leaves the caller
/// anonymous instead of rejecting the request.
#[derive(Debug, Clone)]
pub struct OptionalAuthUser(pub Option<AuthUser>);

impl<S> FromRequestParts<S> for OptionalAuthUser
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        Ok(OptionalAuthUser(
            AuthUser::from_request_parts(parts, state).await.ok(),
        ))
    }
}

/// Current `users.is_super_admin` of a live (not soft-deleted) user.
/// `Ok(None)` when the user does not exist or is deleted.
async fn super_admin_flag(
    db: &DatabaseConnection,
    user_id: Uuid,
) -> Result<Option<bool>, sea_orm::DbErr> {
    let flag: Option<Option<bool>> = users::Entity::find_by_id(user_id)
        .filter(users::Column::DeletedAt.is_null())
        .select_only()
        .column(users::Column::IsSuperAdmin)
        .into_tuple()
        .one(db)
        .await?;
    Ok(flag.map(|flag| flag.unwrap_or(false)))
}

/// echobackend `handler.isSuperAdmin`: whether `user_id` is a super admin
/// right now, read from the database (the JWT claim is ignored so a demoted
/// admin loses access immediately). `false` on any error.
pub async fn is_super_admin(db: &DatabaseConnection, user_id: Uuid) -> bool {
    match super_admin_flag(db, user_id).await {
        Ok(flag) => flag.unwrap_or(false),
        Err(err) => {
            tracing::warn!(%user_id, error = %err, "auth: admin lookup failed");
            false
        }
    }
}

/// Super-admin guard, mirroring echobackend's `Auth()` + `AuthAdmin()`:
/// the `AuthUser` checks (401), then the user's row is loaded (`deleted_at IS
/// NULL`): a DB error or a missing/deleted user is 401 "Failed to validate
/// privileges", `is_super_admin` NULL/false is 403 "Insufficient
/// privileges". The `is_super_admin` claim is ignored.
#[derive(Debug, Clone)]
pub struct AdminUser(pub AuthUser);

impl<S> FromRequestParts<S> for AdminUser
where
    S: Send + Sync,
    DbPool: FromRef<S>,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let mut auth_user = AuthUser::from_request_parts(parts, state).await?;
        let db = DbPool::from_ref(state);
        match super_admin_flag(&db, auth_user.id).await {
            Ok(Some(true)) => {
                auth_user.is_super_admin = true;
                Ok(AdminUser(auth_user))
            }
            Ok(Some(false)) => {
                tracing::warn!(user_id = %auth_user.id, "auth: insufficient privileges");
                Err(auth_error(StatusCode::FORBIDDEN, "Insufficient privileges"))
            }
            Ok(None) => Err(auth_error(
                StatusCode::UNAUTHORIZED,
                "Failed to validate privileges",
            )),
            Err(err) => {
                tracing::warn!(user_id = %auth_user.id, error = %err, "auth: failed to validate admin privileges");
                Err(auth_error(
                    StatusCode::UNAUTHORIZED,
                    "Failed to validate privileges",
                ))
            }
        }
    }
}
