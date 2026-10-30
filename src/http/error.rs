use crate::http::response::ApiResponse;
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use sea_orm::DbErr;

/// Every handler error. Rendering follows echobackend's `pkg/response`:
///
/// | variant | status | `message` | `error` |
/// |---|---|---|---|
/// | `Database` | 500 | "Internal server error" | omitted (never leaked) |
/// | `InternalServerError(m)` | 500 | `m` | omitted |
/// | `NotFound(d)` | 404 | `d` | `d` |
/// | `BadRequest(d)` | 400 | `d` | `d` |
/// | `Unauthorized(m)` | 401 | `m` | "Unauthorized access" |
/// | `Forbidden(m)` | 403 | `m` | "Access forbidden" |
/// | `Conflict(d)` | 409 | `d` | `d` |
/// | `PayloadTooLarge(m)` | 413 | `m` | `m` |
/// | `UnprocessableEntity` | 422 | "Validation failed" | first field message, plus `errors[]` |
/// | `Detailed{..}` | any | `message` | `error` (omitted when `None`/empty) |
///
/// echobackend's handlers pass a handler-specific message plus the domain
/// error (`respondError(c, "Failed to get post views", err)`); use
/// [`AppError::context`] (or the `bad_request` / `not_found` / `conflict`
/// constructors) to reproduce that split.
#[derive(Debug)]
pub enum AppError {
    Database(DbErr),
    NotFound(String),
    BadRequest(String),
    Unauthorized(String),
    Forbidden(String),
    Conflict(String),
    /// Upload body exceeds the size cap (`413`).
    PayloadTooLarge(String),
    /// Validation failure (`422`) carrying structured field errors, mirroring
    /// echobackend's `response.FromValidateError` envelope.
    UnprocessableEntity {
        error: String,
        errors: Vec<crate::http::extract::FieldError>,
    },
    /// `500` with a handler-specific message and no `error` key.
    InternalServerError(String),
    /// Explicit status, message and optional `error` detail.
    Detailed {
        status: StatusCode,
        message: String,
        error: Option<String>,
    },
}

impl AppError {
    /// `422` for content rejected by `crate::domain::antispam`. `reason` is attached
    /// to `field` so the author learns which rule tripped.
    pub fn spam_rejected(error: &str, field: &str, reason: String) -> Self {
        AppError::UnprocessableEntity {
            error: error.to_string(),
            errors: vec![crate::http::extract::FieldError {
                field: field.to_string(),
                message: reason,
                value: None,
                tag: Some("spam".to_string()),
            }],
        }
    }

    /// echobackend `response.BadRequest(c, message, err)`: `error` is the
    /// detail, omitted when `None` or empty.
    pub fn bad_request(message: impl Into<String>, error: Option<String>) -> Self {
        Self::detailed(StatusCode::BAD_REQUEST, message, error)
    }

    /// echobackend `response.NotFound(c, message, err)`: `error` is the
    /// detail, or "Resource not found" when there is none.
    pub fn not_found(message: impl Into<String>, error: Option<String>) -> Self {
        let error = error
            .filter(|e| !e.is_empty())
            .unwrap_or_else(|| "Resource not found".to_string());
        Self::detailed(StatusCode::NOT_FOUND, message, Some(error))
    }

    /// echobackend `response.Conflict(c, message, conflictError)`.
    pub fn conflict(message: impl Into<String>, error: impl Into<String>) -> Self {
        Self::detailed(StatusCode::CONFLICT, message, Some(error.into()))
    }

    /// `500` with `message` and no `error` key (echobackend
    /// `response.InternalServerError`). The cause is logged, never sent.
    pub fn internal(message: impl Into<String>, cause: impl std::fmt::Display) -> Self {
        let message = message.into();
        tracing::error!(%message, error = %cause, "internal server error");
        AppError::InternalServerError(message)
    }

    fn detailed(status: StatusCode, message: impl Into<String>, error: Option<String>) -> Self {
        AppError::Detailed {
            status,
            message: message.into(),
            error: error.filter(|e| !e.is_empty()),
        }
    }

    /// Replaces the human-readable `message` while keeping status and detail,
    /// like echobackend's `respondError(c, message, err)`:
    /// - 500s (`Database`, `InternalServerError`) get `message` and no `error`;
    /// - 404/400/409 keep their detail as `error`;
    /// - 401/403 keep their fixed `error` strings;
    /// - 422 is left untouched ("Validation failed").
    ///
    /// Typical use: `service(..).await.map_err(|e| AppError::from(e).context("Failed to get post views"))?`.
    pub fn context(self, message: impl Into<String>) -> Self {
        let message = message.into();
        match self {
            AppError::Database(err) => {
                tracing::error!(%message, error = ?err, "database error");
                AppError::InternalServerError(message)
            }
            AppError::InternalServerError(_) => AppError::InternalServerError(message),
            AppError::NotFound(detail) => Self::not_found(message, Some(detail)),
            AppError::BadRequest(detail) => Self::bad_request(message, Some(detail)),
            AppError::Conflict(detail) => Self::conflict(message, detail),
            AppError::PayloadTooLarge(detail) => {
                Self::detailed(StatusCode::PAYLOAD_TOO_LARGE, message, Some(detail))
            }
            AppError::Unauthorized(_) => AppError::Unauthorized(message),
            AppError::Forbidden(_) => AppError::Forbidden(message),
            err @ AppError::UnprocessableEntity { .. } => err,
            AppError::Detailed { status, error, .. } => AppError::Detailed {
                status,
                message,
                error,
            },
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, body) = match self {
            AppError::Database(e) => {
                tracing::error!("Database error: {:?}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ApiResponse::failure("Internal server error", None),
                )
            }
            AppError::InternalServerError(msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiResponse::failure(msg, None),
            ),
            AppError::NotFound(msg) => {
                (StatusCode::NOT_FOUND, ApiResponse::error(msg.clone(), msg))
            }
            AppError::BadRequest(msg) => (
                StatusCode::BAD_REQUEST,
                ApiResponse::error(msg.clone(), msg),
            ),
            AppError::Unauthorized(msg) => (
                StatusCode::UNAUTHORIZED,
                ApiResponse::error(msg, "Unauthorized access"),
            ),
            AppError::Forbidden(msg) => (
                StatusCode::FORBIDDEN,
                ApiResponse::error(msg, "Access forbidden"),
            ),
            AppError::Conflict(msg) => (StatusCode::CONFLICT, ApiResponse::error(msg.clone(), msg)),
            AppError::PayloadTooLarge(msg) => (
                StatusCode::PAYLOAD_TOO_LARGE,
                ApiResponse::error(msg.clone(), msg),
            ),
            AppError::UnprocessableEntity { error, errors } => (
                StatusCode::UNPROCESSABLE_ENTITY,
                ApiResponse {
                    errors: Some(serde_json::to_value(errors).unwrap_or_default()),
                    ..ApiResponse::error("Validation failed", error)
                },
            ),
            AppError::Detailed {
                status,
                message,
                error,
            } => {
                let error = match status {
                    StatusCode::UNAUTHORIZED => Some("Unauthorized access".to_string()),
                    StatusCode::FORBIDDEN => Some("Access forbidden".to_string()),
                    StatusCode::INTERNAL_SERVER_ERROR => None,
                    _ => error.filter(|e| !e.is_empty()),
                };
                (status, ApiResponse::failure(message, error))
            }
        };

        (status, Json(body)).into_response()
    }
}

impl From<DbErr> for AppError {
    fn from(err: DbErr) -> Self {
        AppError::Database(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::extract::FieldError;

    #[test]
    fn test_app_error_status_codes() {
        assert_eq!(
            AppError::NotFound("Not found".into())
                .into_response()
                .status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            AppError::BadRequest("Bad input".into())
                .into_response()
                .status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            AppError::Unauthorized("Invalid token".into())
                .into_response()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            AppError::Forbidden("Denied".into())
                .into_response()
                .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            AppError::Conflict("Duplicate key".into())
                .into_response()
                .status(),
            StatusCode::CONFLICT
        );
        assert_eq!(
            AppError::InternalServerError("Oops".into())
                .into_response()
                .status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(
            AppError::Database(DbErr::Custom("db error".into()))
                .into_response()
                .status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(
            AppError::UnprocessableEntity {
                error: "Invalid input".into(),
                errors: vec![FieldError {
                    field: "Username".into(),
                    message: "Username is required".into(),
                    value: None,
                    tag: Some("required".into()),
                }],
            }
            .into_response()
            .status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }

    /// The frontend reads the rejection reason from `errors[].tag == "spam"`;
    /// this pins the envelope it depends on (shared with echobackend).
    #[tokio::test]
    async fn spam_rejected_envelope_carries_reason() {
        let response = AppError::spam_rejected(
            "Content rejected by anti-spam policy",
            "text",
            "contains banned keyword \"gacor\"".to_string(),
        )
        .into_response();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        let body: serde_json::Value = serde_json::from_slice(&bytes).expect("json body");
        assert_eq!(body["message"], "Validation failed");
        assert_eq!(body["error"], "Content rejected by anti-spam policy");
        assert_eq!(
            body["errors"],
            serde_json::json!([{
                "field": "text",
                "message": "contains banned keyword \"gacor\"",
                "tag": "spam",
            }])
        );
    }

    async fn body_of(err: AppError) -> (StatusCode, serde_json::Value) {
        let response = err.into_response();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        (status, serde_json::from_slice(&bytes).expect("json body"))
    }

    /// Pins the echobackend `pkg/response` contract.
    #[tokio::test]
    async fn envelopes_match_echobackend() {
        let (status, body) = body_of(AppError::Unauthorized("Invalid credentials".into())).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(
            body,
            serde_json::json!({"success": false, "message": "Invalid credentials", "error": "Unauthorized access"})
        );

        let (_, body) = body_of(AppError::Forbidden("Not the author".into())).await;
        assert_eq!(body["error"], "Access forbidden");

        let (status, body) = body_of(AppError::Database(DbErr::Custom("dsn leak".into()))).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            body,
            serde_json::json!({"success": false, "message": "Internal server error"})
        );

        let (_, body) = body_of(AppError::InternalServerError(
            "Failed to upload image".into(),
        ))
        .await;
        assert_eq!(
            body,
            serde_json::json!({"success": false, "message": "Failed to upload image"})
        );

        let (_, body) = body_of(AppError::not_found("Failed to get post", None)).await;
        assert_eq!(body["error"], "Resource not found");

        let (_, body) = body_of(AppError::bad_request("Post ID is required", None)).await;
        assert_eq!(
            body,
            serde_json::json!({"success": false, "message": "Post ID is required"})
        );
    }

    #[tokio::test]
    async fn context_keeps_status_and_detail() {
        let (status, body) = body_of(
            AppError::NotFound("post not found".into()).context("Failed to get post views"),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["message"], "Failed to get post views");
        assert_eq!(body["error"], "post not found");

        let (status, body) = body_of(
            AppError::Database(DbErr::Custom("boom".into())).context("Failed to get post views"),
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            body,
            serde_json::json!({"success": false, "message": "Failed to get post views"})
        );

        let (status, body) =
            body_of(AppError::Forbidden("x".into()).context("Failed to get post views")).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["message"], "Failed to get post views");
        assert_eq!(body["error"], "Access forbidden");
    }

    #[test]
    fn test_from_db_err() {
        let db_err = DbErr::RecordNotFound("not found".into());
        let app_err = AppError::from(db_err);
        match app_err {
            AppError::Database(DbErr::RecordNotFound(_)) => {}
            _ => panic!("Expected AppError::Database"),
        }
    }
}
