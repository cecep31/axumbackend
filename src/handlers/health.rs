use crate::database::DbPool;
use axum::{Json, Router, extract::State, http::StatusCode, routing::get};
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

pub async fn root() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "success": true,
        "message": "Hello, World!"
    }))
}

pub async fn health(State(pool): State<DbPool>) -> (StatusCode, Json<HealthResponse>) {
    match pool
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT 1".to_string(),
        ))
        .await
    {
        Ok(_) => (
            StatusCode::OK,
            Json(HealthResponse {
                status: "ok".to_string(),
                reason: None,
            }),
        ),
        Err(err) => {
            tracing::error!("health check failed: {:?}", err);
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(HealthResponse {
                    status: "unhealthy".to_string(),
                    reason: Some("database unreachable".to_string()),
                }),
            )
        }
    }
}

pub fn routes() -> Router<DbPool> {
    Router::new()
        .route("/", get(root))
        .route("/health", get(health))
}
