use serde::{Deserialize, Serialize};

/// A single corporate action event (dividend or RUPS) returned by the calendar
/// endpoint. Mirrors echobackend's `dto.CorporateActionResponse`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorporateActionResponse {
    pub symbol: String,
    pub name: String,
    /// `"dividend"` | `"rups"`
    #[serde(rename = "type")]
    pub action_type: String,
    /// ISO 8601 date, e.g. `"2025-07-15"`
    pub date: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pay_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// `"IDX"` | `"US"`
    pub market: String,
}

/// Top-level response for `GET /api/holdings/calendar`. Mirrors echobackend's
/// `dto.CorporateActionCalendarResponse`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorporateActionCalendarResponse {
    pub from: String,
    pub to: String,
    pub total: usize,
    pub cached: bool,
    pub actions: Vec<CorporateActionResponse>,
}
