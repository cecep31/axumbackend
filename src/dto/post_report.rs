use garde::Validate;
use serde::Deserialize;

#[derive(Deserialize, Validate)]
pub struct CreatePostReportRequest {
    #[garde(length(chars, min = 1, max = 32))]
    pub reason: String,
    #[garde(inner(length(chars, max = 1000)))]
    pub details: Option<String>,
}

#[derive(Deserialize, Validate)]
pub struct ModeratePostRequest {
    #[garde(length(chars, min = 1, max = 16))]
    pub action: String,
    #[garde(inner(length(chars, max = 1000)))]
    pub note: Option<String>,
}

#[derive(Deserialize, Validate)]
pub struct ReportStatusQuery {
    #[garde(skip)]
    pub status: Option<String>,
    #[garde(skip)]
    pub limit: Option<i64>,
    #[garde(skip)]
    pub offset: Option<i64>,
}

impl ReportStatusQuery {
    pub fn status_or_default(&self) -> String {
        let s = self.status.as_deref().unwrap_or("").trim();
        if s.is_empty() {
            "pending".to_string()
        } else {
            s.to_string()
        }
    }

    pub fn limit_or(&self, default: i64) -> u64 {
        let v = self.limit.unwrap_or(default);
        v.clamp(1, 100) as u64
    }

    pub fn offset_or(&self) -> u64 {
        self.offset.unwrap_or(0).max(0) as u64
    }
}
