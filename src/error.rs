use crate::response::ApiResponse;
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use sea_orm::DbErr;

#[derive(Debug)]
#[allow(dead_code)]
pub enum AppError {
    Database(DbErr),
    NotFound(String),
    BadRequest(String),
    Unauthorized(String),
    Forbidden(String),
    Conflict(String),
    /// Validation failure (`422`) carrying structured field errors, mirroring
    /// echobackend's `response.FromValidateError` envelope.
    UnprocessableEntity {
        error: String,
        errors: Vec<crate::extract::FieldError>,
    },
    InternalServerError(String),
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
            AppError::UnprocessableEntity { error, errors } => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "Validation failed".to_string(),
                Some((error, errors)),
            ),
            AppError::InternalServerError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg, None),
        };

        let (error_field, errors_field) = match errors {
            Some((error, errors)) => (
                error,
                Some(serde_json::to_value(errors).unwrap_or_default()),
            ),
            None => (error_message.clone(), None),
        };

        let body = Json(ApiResponse::<serde_json::Value> {
            success: false,
            message: error_message,
            data: None,
            error: Some(error_field),
            errors: errors_field,
            meta: None,
        });

        (status, body).into_response()
    }
}

impl From<DbErr> for AppError {
    fn from(err: DbErr) -> Self {
        AppError::Database(err)
    }
}
