use serde::{Deserialize, Serialize};

/// `GET /health` body (not wrapped in the `ApiResponse` envelope).
#[derive(Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}
