use chrono::{DateTime, FixedOffset, Utc};
use sea_orm::prelude::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone)]
pub struct HoldingTypeResponse {
    pub id: i16,
    pub code: String,
    pub name: String,
    pub notes: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct HoldingResponse {
    pub id: i64,
    pub user_id: Uuid,
    pub name: String,
    pub symbol: Option<String>,
    pub platform: String,
    pub holding_type_id: i16,
    pub holding_type: Option<HoldingTypeResponse>,
    pub currency: String,
    pub invested_amount: String,
    pub current_value: String,
    pub gain_amount: String,
    pub gain_percent: String,
    pub units: Option<String>,
    pub avg_buy_price: Option<String>,
    pub current_price: Option<String>,
    pub last_updated: Option<DateTime<Utc>>,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub month: i32,
    pub year: i32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HoldingSummaryResponse {
    pub total_invested: String,
    pub total_current_value: String,
    pub total_profit_loss: String,
    pub total_profit_loss_percentage: String,
    pub holdings_count: i64,
    pub type_breakdown: Vec<HoldingNamedStringBreakdown>,
    pub platform_breakdown: Vec<HoldingNamedStringBreakdown>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct HoldingNamedStringBreakdown {
    pub name: String,
    pub invested: String,
    pub current: String,
    pub profit_loss: String,
    pub profit_loss_percentage: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HoldingTrendResponse {
    pub date: String,
    pub invested: String,
    pub current: String,
    pub profit_loss: String,
    pub profit_loss_percentage: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HoldingMonthPoint {
    pub month: i32,
    pub year: i32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HoldingBreakdownValues {
    pub invested: f64,
    pub current: f64,
    pub profit_loss: f64,
    pub profit_loss_percentage: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HoldingNamedBreakdown {
    pub name: String,
    pub invested: f64,
    pub current: f64,
    pub profit_loss: f64,
    pub profit_loss_percentage: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HoldingSummaryValues {
    pub total_invested: f64,
    pub total_current_value: f64,
    pub total_profit_loss: f64,
    pub total_profit_loss_percentage: f64,
    pub holdings_count: i64,
    pub type_breakdown: Vec<HoldingNamedBreakdown>,
    pub platform_breakdown: Vec<HoldingNamedBreakdown>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HoldingCompareSummary {
    pub from: HoldingSummaryValues,
    pub to: HoldingSummaryValues,
    pub invested_diff: f64,
    pub current_value_diff: f64,
    pub profit_loss_diff: f64,
    pub holdings_count_diff: i64,
    pub invested_diff_percentage: f64,
    pub current_value_diff_percentage: f64,
    pub holdings_count_diff_percentage: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HoldingCompareBreakdown {
    pub name: String,
    pub from: HoldingBreakdownValues,
    pub to: HoldingBreakdownValues,
    pub invested_diff: f64,
    pub current_value_diff: f64,
    pub profit_loss_diff: f64,
    pub invested_diff_percentage: f64,
    pub current_value_diff_percentage: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HoldingMonthComparisonResponse {
    pub from_month: HoldingMonthPoint,
    pub to_month: HoldingMonthPoint,
    pub summary: HoldingCompareSummary,
    pub type_comparison: Vec<HoldingCompareBreakdown>,
    pub platform_comparison: Vec<HoldingCompareBreakdown>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HoldingMonthlyDataResponse {
    pub month: i32,
    pub year: i32,
    pub date: String,
    pub total_current_value: String,
    pub total_invested: String,
    pub holdings_count: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HoldingSyncResponse {
    pub synced_count: i64,
    pub month: i32,
    pub year: i32,
}

#[derive(Serialize)]
pub struct DuplicateResultItem {
    pub id: String,
    pub name: String,
    pub month: i32,
    pub year: i32,
}

/// Preserves the declared column scale (e.g. `numeric(18,2)`) instead of
/// stripping trailing zeros, matching echobackend's raw Postgres `numeric`
/// text output.
fn decimal_to_string(value: Decimal) -> String {
    value.to_string()
}

fn maybe_decimal_to_string(value: Option<Decimal>) -> Option<String> {
    value.map(decimal_to_string)
}

fn to_utc(value: DateTime<FixedOffset>) -> DateTime<Utc> {
    value.with_timezone(&Utc)
}

fn maybe_to_utc(value: Option<DateTime<FixedOffset>>) -> Option<DateTime<Utc>> {
    value.map(|dt| dt.with_timezone(&Utc))
}

impl From<crate::entities::holding_types::Model> for HoldingTypeResponse {
    fn from(model: crate::entities::holding_types::Model) -> Self {
        Self {
            id: model.id,
            code: model.code,
            name: model.name,
            notes: model.notes,
        }
    }
}

impl HoldingResponse {
    pub fn from_entity(
        holding: crate::entities::holdings::Model,
        holding_type: Option<crate::entities::holding_types::Model>,
    ) -> Self {
        let gain_amount = holding
            .gain_amount
            .unwrap_or(holding.current_value - holding.invested_amount);
        let gain_percent = holding.gain_percent.unwrap_or_else(|| {
            if holding.invested_amount.is_zero() {
                Decimal::ZERO
            } else {
                ((holding.current_value - holding.invested_amount) / holding.invested_amount)
                    * Decimal::new(100, 0)
            }
        });

        Self {
            id: holding.id,
            user_id: holding.user_id,
            name: holding.name,
            symbol: holding.symbol,
            platform: holding.platform,
            holding_type_id: holding.holding_type_id,
            holding_type: holding_type.map(Into::into),
            currency: holding.currency,
            invested_amount: decimal_to_string(holding.invested_amount),
            current_value: decimal_to_string(holding.current_value),
            gain_amount: decimal_to_string(gain_amount),
            gain_percent: decimal_to_string(gain_percent),
            units: maybe_decimal_to_string(holding.units),
            avg_buy_price: maybe_decimal_to_string(holding.avg_buy_price),
            current_price: maybe_decimal_to_string(holding.current_price),
            last_updated: maybe_to_utc(holding.last_updated),
            notes: holding.notes,
            created_at: to_utc(holding.created_at),
            updated_at: to_utc(holding.updated_at),
            month: holding.month,
            year: holding.year,
        }
    }
}
