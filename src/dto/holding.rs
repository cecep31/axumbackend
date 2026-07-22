use crate::dto::validation::{parse_month, parse_year};
use serde::Deserialize;
use validator::Validate;

#[derive(Deserialize, Validate)]
pub struct HoldingPath {
    pub id: i64,
}

#[derive(Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct HoldingQuery {
    /// Kept as a raw string and parsed leniently: only `1..=12` overrides the
    /// caller's default month, mirroring echobackend's inline `QueryParam` parsing.
    pub month: Option<String>,
    /// Kept as a raw string and parsed leniently: any integer is accepted
    /// (no range bound), mirroring echobackend.
    pub year: Option<String>,
    pub sort_by: Option<String>,
    pub order: Option<String>,
}

impl HoldingQuery {
    pub fn month(&self) -> Option<i32> {
        parse_month(self.month.as_deref())
    }

    pub fn year(&self) -> Option<i32> {
        parse_year(self.year.as_deref())
    }
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
    pub month: Option<String>,
    pub year: Option<String>,
}

impl SummaryQuery {
    pub fn month(&self) -> Option<i32> {
        parse_month(self.month.as_deref())
    }

    pub fn year(&self) -> Option<i32> {
        parse_year(self.year.as_deref())
    }
}

#[derive(Deserialize, Validate)]
pub struct TrendsQuery {
    pub years: Option<String>,
}

#[derive(Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct CompareQuery {
    pub from_month: Option<String>,
    pub from_year: Option<String>,
    pub to_month: Option<String>,
    pub to_year: Option<String>,
}

impl CompareQuery {
    pub fn from_month(&self) -> Option<i32> {
        parse_month(self.from_month.as_deref())
    }

    pub fn from_year(&self) -> Option<i32> {
        parse_year(self.from_year.as_deref())
    }

    pub fn to_month(&self) -> Option<i32> {
        parse_month(self.to_month.as_deref())
    }

    pub fn to_year(&self) -> Option<i32> {
        parse_year(self.to_year.as_deref())
    }
}

#[derive(Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct MonthlyQuery {
    pub start_month: Option<String>,
    pub start_year: Option<String>,
    pub end_month: Option<String>,
    pub end_year: Option<String>,
}

impl MonthlyQuery {
    pub fn start_month(&self) -> Option<i32> {
        parse_month(self.start_month.as_deref())
    }

    pub fn start_year(&self) -> Option<i32> {
        parse_year(self.start_year.as_deref())
    }

    pub fn end_month(&self) -> Option<i32> {
        parse_month(self.end_month.as_deref())
    }

    pub fn end_year(&self) -> Option<i32> {
        parse_year(self.end_year.as_deref())
    }
}

/// Query params for `GET /api/holdings/calendar` (corporate-actions calendar),
/// mirroring echobackend's `CorporateActionHandler.GetCalendar`.
#[derive(Deserialize, Validate)]
pub struct CalendarQuery {
    /// Month `1-12`. Defaults to the current month; invalid values also fall
    /// back to the current month (handled by the service, like echobackend).
    pub month: Option<i32>,
    /// Four-digit year. Defaults to the current year.
    pub year: Option<i32>,
}
