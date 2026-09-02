use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, PartialEq)]
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

#[derive(Serialize, Deserialize, Debug, PartialEq)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_meta_default() {
        let meta = Meta::default();
        assert_eq!(meta.total_items, 0);
        assert_eq!(meta.offset, 0);
        assert_eq!(meta.limit, 10);
        assert_eq!(meta.total_pages, 0);
    }

    #[test]
    fn test_success_with_message() {
        let res = ApiResponse::success_with_message("Created successfully", 42);
        assert!(res.success);
        assert_eq!(res.message, "Created successfully");
        assert_eq!(res.data, Some(42));
        assert_eq!(res.error, None);
        assert_eq!(res.errors, None);
        assert_eq!(res.meta, None);
    }

    #[test]
    fn test_with_meta_message_pagination_math() {
        // Exact division
        let res = ApiResponse::with_meta_message("OK", "items", 20, 10, 0);
        let meta = res.meta.unwrap();
        assert_eq!(meta.total_items, 20);
        assert_eq!(meta.limit, 10);
        assert_eq!(meta.offset, 0);
        assert_eq!(meta.total_pages, 2);

        // Ceiling division
        let res = ApiResponse::with_meta_message("OK", "items", 25, 10, 20);
        let meta = res.meta.unwrap();
        assert_eq!(meta.total_items, 25);
        assert_eq!(meta.limit, 10);
        assert_eq!(meta.offset, 20);
        assert_eq!(meta.total_pages, 3);

        // Zero items
        let res = ApiResponse::with_meta_message("OK", "items", 0, 10, 0);
        let meta = res.meta.unwrap();
        assert_eq!(meta.total_pages, 0);

        // Zero limit (avoids division by zero)
        let res = ApiResponse::with_meta_message("OK", "items", 25, 0, 0);
        let meta = res.meta.unwrap();
        assert_eq!(meta.total_pages, 0);
    }

    #[test]
    fn test_serialization_omits_none() {
        let res: ApiResponse<String> = ApiResponse::success_with_message("Hello", "world".into());
        let json_str = serde_json::to_string(&res).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json_str).unwrap();

        assert_eq!(value["success"], true);
        assert_eq!(value["message"], "Hello");
        assert_eq!(value["data"], "world");
        assert!(value.get("error").is_none());
        assert!(value.get("errors").is_none());
        assert!(value.get("meta").is_none());
    }
}
