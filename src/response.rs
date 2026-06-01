use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct Meta {
    pub total_items: i64,
    pub offset: i64,
    pub limit: i64,
    pub total_pages: i64,
}

impl Default for Meta {
    fn default() -> Self {
        Meta {
            total_items: 0,
            offset: 0,
            limit: 10,
            total_pages: 0,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub errors: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<Meta>,
}

impl<T> ApiResponse<T> {
    pub fn success_with_message(message: impl Into<String>, data: T) -> Self {
        ApiResponse {
            success: true,
            message: message.into(),
            data: Some(data),
            error: None,
            errors: None,
            meta: None,
        }
    }

    pub fn with_meta_message(
        message: impl Into<String>,
        data: T,
        total: i64,
        limit: i64,
        offset: i64,
    ) -> Self {
        let total_pages = if limit > 0 {
            (total as f64 / limit as f64).ceil() as i64
        } else {
            0
        };

        ApiResponse {
            success: true,
            message: message.into(),
            data: Some(data),
            error: None,
            errors: None,
            meta: Some(Meta {
                total_items: total,
                offset,
                limit,
                total_pages,
            }),
        }
    }
}
