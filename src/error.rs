use crate::response::ApiResponse;
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use sea_orm::DbErr;

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
        errors: Vec<crate::extract::FieldError>,
    },
    InternalServerError(String),
}

impl AppError {
    /// `422` for content rejected by `crate::antispam`. `reason` is attached
    /// to `field` so the author learns which rule tripped.
    pub fn spam_rejected(error: &str, field: &str, reason: String) -> Self {
        AppError::UnprocessableEntity {
            error: error.to_string(),
            errors: vec![crate::extract::FieldError {
                field: field.to_string(),
                message: reason,
                tag: Some("spam".to_string()),
            }],
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, error_message, errors) = match self {
            AppError::Database(e) => {
                tracing::error!("Database error: {:?}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".to_string(),
                    None,
                )
            }

            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, msg, None),
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg, None),
            AppError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg, None),
            AppError::Forbidden(msg) => (StatusCode::FORBIDDEN, msg, None),
            AppError::Conflict(msg) => (StatusCode::CONFLICT, msg, None),
            AppError::PayloadTooLarge(msg) => (StatusCode::PAYLOAD_TOO_LARGE, msg, None),
            AppError::UnprocessableEntity { error, errors } => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "Validation failed".to_string(),
                Some((error, errors)),
            ),
            AppError::InternalServerError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg, None),
        };

        let body = match errors {
            Some((error, errors)) => ApiResponse {
                errors: Some(serde_json::to_value(errors).unwrap_or_default()),
                ..ApiResponse::error(error_message, error)
            },
            None => ApiResponse::error(error_message.clone(), error_message),
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
    use crate::extract::FieldError;

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
                    field: "username".into(),
                    message: "Username is required".into(),
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
