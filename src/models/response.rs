use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct Meta {
    pub total: i64,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

impl Default for Meta {
    fn default() -> Self {
        Meta {
            total: 0,
            limit: None,
            offset: None,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub meta: Meta,
}

impl<T> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        ApiResponse {
            success: true,
            data: Some(data),
            meta: Meta::default(),
        }
    }

    pub fn with_meta(data: T, total: i64, limit: Option<i64>, offset: Option<i64>) -> Self {
        ApiResponse {
            success: true,
            data: Some(data),
            meta: Meta { total, limit, offset },
        }
    }
}
