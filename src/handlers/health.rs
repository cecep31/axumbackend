use crate::database::DbPool;
use axum::{Json, Router, routing::get};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct HealthResponse {
    pub success: bool,
    pub message: String,
}

pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        success: true,
        message: String::from("helloword"),
    })
}

pub fn routes() -> Router<DbPool> {
    Router::new()
        .route("/", get(health))
        .route("/v1/health", get(health))
}
