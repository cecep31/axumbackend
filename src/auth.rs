use crate::config::JwtConfig;
use crate::error::AppError;
use crate::response::ApiResponse;
use axum::{
    Json,
    extract::FromRequestParts,
    http::{StatusCode, request::Parts},
    response::{IntoResponse, Response},
};
use jsonwebtoken::{DecodingKey, Validation, decode};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub user_id: Uuid,
    pub username: Option<String>,
    pub email: String,
    pub is_super_admin: Option<bool>,
    pub iat: usize,
    pub exp: usize,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct AuthUser {
    pub id: Uuid,
    pub email: String,
    pub username: Option<String>,
    pub is_super_admin: bool,
}

/// Builds the auth-middleware error envelope documented in the API reference:
/// 401 responses use `error: "Unauthorized access"`, 403 responses use
/// `error: "Access forbidden"`, with a more specific human-readable `message`.
fn auth_error(status: StatusCode, message: impl Into<String>) -> Response {
    let message = message.into();
    let error = if status == StatusCode::FORBIDDEN {
        "Access forbidden"
    } else {
        "Unauthorized access"
    };
    (
        status,
        Json(ApiResponse::<serde_json::Value> {
            success: false,
            message,
            data: None,
            error: Some(error.to_string()),
            errors: None,
            meta: None,
        }),
    )
        .into_response()
}

impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
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

        Ok(AuthUser {
            id: token.claims.user_id,
            email: token.claims.email,
            username: token.claims.username,
            is_super_admin: token.claims.is_super_admin.unwrap_or(false),
        })
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct AdminUser(pub AuthUser);

impl<S> FromRequestParts<S> for AdminUser
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let auth_user = AuthUser::from_request_parts(parts, state).await?;
        if !auth_user.is_super_admin {
            return Err(auth_error(StatusCode::FORBIDDEN, "Insufficient privileges"));
        }
        Ok(AdminUser(auth_user))
    }
}

impl From<AuthUser> for AppError {
    fn from(_: AuthUser) -> Self {
        AppError::InternalServerError("unexpected auth conversion".to_string())
    }
}
