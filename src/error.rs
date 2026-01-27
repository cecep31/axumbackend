use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use deadpool_postgres::PoolError;
use serde_json::json;

#[derive(Debug)]
#[allow(dead_code)]
pub enum AppError {
    Database(tokio_postgres::Error),
    Pool(PoolError),
    NotFound(String),
    BadRequest(String),
    InternalServerError(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, error_message) = match self {
            AppError::Database(e) => {
                // Log the full error for debugging
                tracing::error!("Database error: {:?}", e);
                // Include more details in the response for debugging
                let error_msg = if e.to_string().is_empty() {
                    format!("Database error: {:?}", e)
                } else {
                    format!("Database error: {}", e)
                };
                (StatusCode::INTERNAL_SERVER_ERROR, error_msg)
            }
            AppError::Pool(e) => (
                StatusCode::SERVICE_UNAVAILABLE,
                format!("Connection pool error: {}", e),
            ),
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            AppError::InternalServerError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
        };

        let body = Json(json!({
            "success": false,
            "error": error_message,
            "data": serde_json::Value::Null
        }));

        (status, body).into_response()
    }
}

impl From<tokio_postgres::Error> for AppError {
    fn from(err: tokio_postgres::Error) -> Self {
        AppError::Database(err)
    }
}

impl From<PoolError> for AppError {
    fn from(err: PoolError) -> Self {
        AppError::Pool(err)
    }
}
