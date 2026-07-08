use serde::Deserialize;
use validator::Validate;

#[derive(Deserialize, Validate)]
pub struct HoldingPath {
    pub id: i64,
}

#[derive(Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct HoldingQuery {
    #[validate(range(min = 1, max = 12))]
    pub month: Option<i32>,
    #[validate(range(min = 2000))]
    pub year: Option<i32>,
    pub sort_by: Option<String>,
    pub order: Option<String>,
}

#[derive(Deserialize, Validate)]
pub struct CreateHoldingRequest {
    #[validate(length(min = 1))]
    pub name: String,
    pub symbol: Option<String>,
    #[validate(length(min = 1))]
    pub platform: String,
    pub holding_type_id: i16,
    #[validate(length(equal = 3))]
    pub currency: String,
    pub invested_amount: String,
    pub current_value: String,
    pub units: Option<String>,
    pub avg_buy_price: Option<String>,
    pub current_price: Option<String>,
    pub last_updated: Option<String>,
    pub notes: Option<String>,
    #[validate(range(min = 1, max = 12))]
    pub month: i32,
    #[validate(range(min = 2000))]
    pub year: i32,
}

#[derive(Deserialize, Validate)]
pub struct UpdateHoldingRequest {
    #[validate(length(min = 1))]
    pub name: Option<String>,
    pub symbol: Option<String>,
    #[validate(length(min = 1))]
    pub platform: Option<String>,
    pub holding_type_id: Option<i16>,
    #[validate(length(equal = 3))]
    pub currency: Option<String>,
    pub invested_amount: Option<String>,
    pub current_value: Option<String>,
    pub units: Option<String>,
    pub avg_buy_price: Option<String>,
    pub current_price: Option<String>,
    pub last_updated: Option<String>,
    pub notes: Option<String>,
    #[validate(range(min = 1, max = 12))]
    pub month: Option<i32>,
    #[validate(range(min = 2000))]
    pub year: Option<i32>,
}

#[derive(Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateHoldingRequest {
    #[validate(range(min = 1, max = 12))]
    pub from_month: i32,
    #[validate(range(min = 1900, max = 2100))]
    pub from_year: i32,
    #[validate(range(min = 1, max = 12))]
    pub to_month: i32,
    #[validate(range(min = 1900, max = 2100))]
    pub to_year: i32,
    #[serde(default)]
    pub overwrite: bool,
}

#[derive(Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct SummaryQuery {
    #[validate(range(min = 1, max = 12))]
    pub month: Option<i32>,
    #[validate(range(min = 2000))]
    pub year: Option<i32>,
}

#[derive(Deserialize, Validate)]
pub struct TrendsQuery {
    pub years: Option<String>,
}

#[derive(Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct CompareQuery {
    #[validate(range(min = 1, max = 12))]
    pub from_month: Option<i32>,
    pub from_year: Option<i32>,
    #[validate(range(min = 1, max = 12))]
    pub to_month: Option<i32>,
    pub to_year: Option<i32>,
}

#[derive(Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct MonthlyQuery {
    #[validate(range(min = 1, max = 12))]
    pub start_month: Option<i32>,
    pub start_year: Option<i32>,
    #[validate(range(min = 1, max = 12))]
    pub end_month: Option<i32>,
    pub end_year: Option<i32>,
}

/// Query params for `GET /api/holdings/calendar` (corporate-actions calendar).
/// `from`/`to` are optional `YYYY-MM-DD` strings.
#[derive(Deserialize, Validate)]
pub struct CalendarQuery {
    /// Inclusive start date (`YYYY-MM-DD`). Defaults to the first day of the
    /// current month.
    pub from: Option<String>,
    /// Inclusive end date (`YYYY-MM-DD`). Defaults to 3 months from today.
    pub to: Option<String>,
}
