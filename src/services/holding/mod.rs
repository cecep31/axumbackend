mod analytics;
mod crud;
mod prices;

use chrono::DateTime;
use sea_orm::DbErr;
use sea_orm::prelude::Decimal;
use std::str::FromStr;

pub use analytics::{
    compare_months, default_current_month_year, monthly_data, prev_month, prev_n_months, summary,
    trends,
};
pub use crud::{
    create_holding, delete_holding, duplicate_holdings, get_holding_by_id, get_holding_types,
    get_holdings, update_holding,
};
pub use prices::sync_prices;

#[derive(Debug)]
pub enum HoldingError {
    Db(DbErr),
    NotFound,
    HoldingTypeNotFound,
    InvalidDecimal(&'static str),
    DuplicateSameMonth,
    InvalidRange,
    /// More than 240 months requested (echobackend `ErrHoldingRangeTooLarge`).
    RangeTooLarge,
    Quote(String),
}

impl From<DbErr> for HoldingError {
    fn from(err: DbErr) -> Self {
        Self::Db(err)
    }
}

#[derive(Clone)]
pub struct CreateHoldingInput {
    pub name: String,
    pub symbol: Option<String>,
    pub platform: String,
    pub holding_type_id: i16,
    pub currency: String,
    pub invested_amount: String,
    pub current_value: String,
    pub units: Option<String>,
    pub avg_buy_price: Option<String>,
    pub current_price: Option<String>,
    pub last_updated: Option<String>,
    pub notes: Option<String>,
    pub month: i32,
    pub year: i32,
}

#[derive(Clone)]
pub struct UpdateHoldingInput {
    pub name: Option<String>,
    pub symbol: Option<String>,
    pub platform: Option<String>,
    pub holding_type_id: Option<i16>,
    pub currency: Option<String>,
    pub invested_amount: Option<String>,
    pub current_value: Option<String>,
    pub units: Option<String>,
    pub avg_buy_price: Option<String>,
    pub current_price: Option<String>,
    pub last_updated: Option<String>,
    pub notes: Option<String>,
    pub month: Option<i32>,
    pub year: Option<i32>,
}

#[derive(Clone, Copy)]
struct SummaryValues {
    invested: f64,
    current: f64,
    count: i64,
}

#[derive(Clone)]
struct BreakdownValues {
    name: String,
    invested: f64,
    current: f64,
}

fn parse_decimal(raw: &str, field: &'static str) -> Result<Decimal, HoldingError> {
    Decimal::from_str(raw.trim()).map_err(|_| HoldingError::InvalidDecimal(field))
}

fn parse_optional_decimal(
    raw: Option<String>,
    field: &'static str,
) -> Result<Option<Decimal>, HoldingError> {
    raw.map(|value| parse_decimal(&value, field)).transpose()
}

fn parse_optional_time(raw: Option<String>) -> Option<DateTime<chrono::FixedOffset>> {
    raw.and_then(|value| DateTime::parse_from_rfc3339(&value).ok())
}

/// Mirrors echobackend's `formatFloat` (`strconv.FormatFloat(f, 'f', -1, 64)`):
/// full float precision, no rounding, shortest round-trip representation.
fn format_float(value: f64) -> String {
    if value == 0.0 {
        return "0".to_string();
    }
    value.to_string()
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn calc_percent(base: f64, value: f64) -> f64 {
    if base == 0.0 {
        0.0
    } else {
        round2(((value - base) / base) * 100.0)
    }
}

fn calc_percent_i64(base: i64, value: i64) -> f64 {
    if base == 0 {
        0.0
    } else {
        round2(((value - base) as f64 / base as f64) * 100.0)
    }
}
