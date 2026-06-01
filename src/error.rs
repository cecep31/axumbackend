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
    InternalServerError(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, error_message) = match self {
            AppError::Database(e) => {
                tracing::error!("Database error: {:?}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".to_string(),
                )
            }

            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            AppError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg),
            AppError::Forbidden(msg) => (StatusCode::FORBIDDEN, msg),
            AppError::InternalServerError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
        };

        let body = Json(ApiResponse::<serde_json::Value> {
            success: false,
            message: error_message.clone(),
            data: None,
            error: Some(error_message),
            errors: None,
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
