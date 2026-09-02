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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::FieldError;

    #[test]
    fn test_app_error_status_codes() {
        assert_eq!(
            AppError::NotFound("Not found".into()).into_response().status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            AppError::BadRequest("Bad input".into()).into_response().status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            AppError::Unauthorized("Invalid token".into()).into_response().status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            AppError::Forbidden("Denied".into()).into_response().status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            AppError::Conflict("Duplicate key".into()).into_response().status(),
            StatusCode::CONFLICT
        );
        assert_eq!(
            AppError::InternalServerError("Oops".into()).into_response().status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(
            AppError::Database(DbErr::Custom("db error".into())).into_response().status(),
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
