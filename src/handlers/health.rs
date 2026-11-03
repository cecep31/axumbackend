use crate::http::response::ApiResponse;
use crate::infra::database::DbPool;
use crate::models::health::HealthResponse;
use crate::state::AppState;
use axum::{Json, Router, extract::State, http::StatusCode, routing::get};
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

/// `GET /` — payload-less success envelope `{ success: true, message }`.
pub async fn root() -> Json<ApiResponse<()>> {
    Json(ApiResponse::message("Hello, World!"))
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

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(root))
        .route("/health", get(health))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn root_body_matches_previous_literal() {
        let Json(body) = root().await;
        assert_eq!(
            serde_json::to_value(&body).unwrap(),
            serde_json::json!({
                "success": true,
                "message": "Hello, World!"
            })
        );
    }
}
