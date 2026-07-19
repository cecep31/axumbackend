use crate::entities::{holding_types, holdings};
use crate::models::holding::*;
use chrono::{DateTime, Datelike, Utc};
use sea_orm::prelude::Decimal;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbBackend, DbErr, EntityTrait,
    FromQueryResult, IntoActiveModel, ModelTrait, QueryFilter, QueryOrder, Set, Statement,
};
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use uuid::Uuid;

#[derive(Debug)]
pub enum HoldingError {
    Db(DbErr),
    NotFound,
    HoldingTypeNotFound,
    InvalidDecimal(&'static str),
    DuplicateSameMonth,
    InvalidRange,
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

async fn hydrate(
    db: &DatabaseConnection,
    holding: holdings::Model,
) -> Result<HoldingResponse, DbErr> {
    let holding_type = holding
        .clone()
        .find_related(holding_types::Entity)
        .one(db)
        .await?;
    Ok(HoldingResponse::from_entity(holding, holding_type))
}

async fn holding_type_exists(db: &DatabaseConnection, id: i16) -> Result<bool, DbErr> {
    Ok(holding_types::Entity::find_by_id(id)
        .one(db)
        .await?
        .is_some())
}

pub async fn get_holding_types(
    db: &DatabaseConnection,
) -> Result<Vec<HoldingTypeResponse>, HoldingError> {
    let types = holding_types::Entity::find()
        .order_by_asc(holding_types::Column::Id)
        .all(db)
        .await?;
    Ok(types.into_iter().map(Into::into).collect())
}

pub async fn get_holdings(
    db: &DatabaseConnection,
    user_id: Uuid,
    month: Option<i32>,
    year: Option<i32>,
    sort_by: Option<&str>,
    order: Option<&str>,
) -> Result<Vec<HoldingResponse>, HoldingError> {
    let mut query = holdings::Entity::find().filter(holdings::Column::UserId.eq(user_id));
    if let Some(month) = month {
        query = query.filter(holdings::Column::Month.eq(month));
    }
    if let Some(year) = year {
        query = query.filter(holdings::Column::Year.eq(year));
    }

    let desc = !matches!(order, Some("asc"));
    let sort_field = sort_by.unwrap_or("created_at");
    query = match sort_field {
        "updated_at" => {
            if desc {
                query.order_by_desc(holdings::Column::UpdatedAt)
            } else {
                query.order_by_asc(holdings::Column::UpdatedAt)
            }
        }
        "name" => {
            if desc {
                query.order_by_desc(holdings::Column::Name)
            } else {
                query.order_by_asc(holdings::Column::Name)
            }
        }
        "platform" => {
            if desc {
                query.order_by_desc(holdings::Column::Platform)
            } else {
                query.order_by_asc(holdings::Column::Platform)
            }
        }
        "invested_amount" => {
            if desc {
                query.order_by_desc(holdings::Column::InvestedAmount)
            } else {
                query.order_by_asc(holdings::Column::InvestedAmount)
            }
        }
        "current_value" => {
            if desc {
                query.order_by_desc(holdings::Column::CurrentValue)
            } else {
                query.order_by_asc(holdings::Column::CurrentValue)
            }
        }
        "holding_type" => {
            if desc {
                query.order_by_desc(holdings::Column::HoldingTypeId)
            } else {
                query.order_by_asc(holdings::Column::HoldingTypeId)
            }
        }
        _ => {
            if desc {
                query.order_by_desc(holdings::Column::CreatedAt)
            } else {
                query.order_by_asc(holdings::Column::CreatedAt)
            }
        }
    };
    // echobackend tiebreaks the `holding_type` sort on `created_at`; every
    // other field tiebreaks on `id`.
    query = if sort_field == "holding_type" {
        if desc {
            query.order_by_desc(holdings::Column::CreatedAt)
        } else {
            query.order_by_asc(holdings::Column::CreatedAt)
        }
    } else if desc {
        query.order_by_desc(holdings::Column::Id)
    } else {
        query.order_by_asc(holdings::Column::Id)
    };

    let models = query.all(db).await?;
    let mut responses = Vec::with_capacity(models.len());
    for model in models {
        responses.push(hydrate(db, model).await?);
    }
    Ok(responses)
}

pub async fn get_holding_by_id(
    db: &DatabaseConnection,
    id: i64,
    user_id: Uuid,
) -> Result<HoldingResponse, HoldingError> {
    let Some(model) = holdings::Entity::find_by_id(id)
        .filter(holdings::Column::UserId.eq(user_id))
        .one(db)
        .await?
    else {
        return Err(HoldingError::NotFound);
    };
    Ok(hydrate(db, model).await?)
}

pub async fn create_holding(
    db: &DatabaseConnection,
    user_id: Uuid,
    input: CreateHoldingInput,
) -> Result<HoldingResponse, HoldingError> {
    if !holding_type_exists(db, input.holding_type_id).await? {
        return Err(HoldingError::HoldingTypeNotFound);
    }

    let now = Utc::now().into();
    let model = holdings::ActiveModel {
        user_id: Set(user_id),
        name: Set(input.name),
        symbol: Set(input.symbol),
        platform: Set(input.platform),
        holding_type_id: Set(input.holding_type_id),
        currency: Set(input.currency),
        invested_amount: Set(parse_decimal(&input.invested_amount, "invested_amount")?),
        current_value: Set(parse_decimal(&input.current_value, "current_value")?),
        units: Set(parse_optional_decimal(input.units, "units")?),
        avg_buy_price: Set(parse_optional_decimal(
            input.avg_buy_price,
            "avg_buy_price",
        )?),
        current_price: Set(parse_optional_decimal(
            input.current_price,
            "current_price",
        )?),
        last_updated: Set(parse_optional_time(input.last_updated)),
        notes: Set(input.notes),
        created_at: Set(now),
        updated_at: Set(now),
        month: Set(input.month),
        year: Set(input.year),
        ..Default::default()
    }
    .insert(db)
    .await?;

    Ok(hydrate(db, model).await?)
}

pub async fn update_holding(
    db: &DatabaseConnection,
    id: i64,
    user_id: Uuid,
    input: UpdateHoldingInput,
) -> Result<HoldingResponse, HoldingError> {
    let Some(existing) = holdings::Entity::find_by_id(id)
        .filter(holdings::Column::UserId.eq(user_id))
        .one(db)
        .await?
    else {
        return Err(HoldingError::NotFound);
    };

    if let Some(holding_type_id) = input.holding_type_id
        && !holding_type_exists(db, holding_type_id).await?
    {
        return Err(HoldingError::HoldingTypeNotFound);
    }

    let mut active = existing.into_active_model();
    if let Some(value) = input.name {
        active.name = Set(value);
    }
    if input.symbol.is_some() {
        active.symbol = Set(input.symbol);
    }
    if let Some(value) = input.platform {
        active.platform = Set(value);
    }
    if let Some(value) = input.holding_type_id {
        active.holding_type_id = Set(value);
    }
    if let Some(value) = input.currency {
        active.currency = Set(value);
    }
    if let Some(value) = input.invested_amount {
        active.invested_amount = Set(parse_decimal(&value, "invested_amount")?);
    }
    if let Some(value) = input.current_value {
        active.current_value = Set(parse_decimal(&value, "current_value")?);
    }
    if input.units.is_some() {
        active.units = Set(parse_optional_decimal(input.units, "units")?);
    }
    if input.avg_buy_price.is_some() {
        active.avg_buy_price = Set(parse_optional_decimal(
            input.avg_buy_price,
            "avg_buy_price",
        )?);
    }
    if input.current_price.is_some() {
        active.current_price = Set(parse_optional_decimal(
            input.current_price,
            "current_price",
        )?);
    }
    if input.last_updated.is_some() {
        active.last_updated = Set(parse_optional_time(input.last_updated));
    }
    if input.notes.is_some() {
        active.notes = Set(input.notes);
    }
    if let Some(value) = input.month {
        active.month = Set(value);
    }
    if let Some(value) = input.year {
        active.year = Set(value);
    }
    active.updated_at = Set(Utc::now().into());

    let updated = active.update(db).await?;
    Ok(hydrate(db, updated).await?)
}

pub async fn delete_holding(
    db: &DatabaseConnection,
    id: i64,
    user_id: Uuid,
) -> Result<(), HoldingError> {
    let result = holdings::Entity::delete_many()
        .filter(holdings::Column::Id.eq(id))
        .filter(holdings::Column::UserId.eq(user_id))
        .exec(db)
        .await?;
    if result.rows_affected == 0 {
        return Err(HoldingError::NotFound);
    }
    Ok(())
}

async fn summary_values(
    db: &DatabaseConnection,
    user_id: Uuid,
    month: Option<i32>,
    year: Option<i32>,
) -> Result<SummaryValues, DbErr> {
    let month_clause = month
        .map(|v| format!(" AND month = {}", v))
        .unwrap_or_default();
    let year_clause = year
        .map(|v| format!(" AND year = {}", v))
        .unwrap_or_default();
    #[derive(FromQueryResult)]
    struct Row {
        invested: f64,
        current_value: f64,
        count: i64,
    }
    let row = Row::find_by_statement(Statement::from_string(
        DbBackend::Postgres,
        format!(
            "SELECT COALESCE(SUM(invested_amount), 0)::float8 AS invested, COALESCE(SUM(current_value), 0)::float8 AS current_value, COUNT(*)::bigint AS count FROM holdings WHERE user_id = '{}'{}{}",
            user_id, month_clause, year_clause
        ),
    ))
    .one(db)
    .await?
    .unwrap_or(Row {
        invested: 0.0,
        current_value: 0.0,
        count: 0,
    });
    Ok(SummaryValues {
        invested: row.invested,
        current: row.current_value,
        count: row.count,
    })
}

async fn named_breakdown(
    db: &DatabaseConnection,
    user_id: Uuid,
    month: Option<i32>,
    year: Option<i32>,
    by_type: bool,
) -> Result<Vec<BreakdownValues>, DbErr> {
    #[derive(FromQueryResult)]
    struct Row {
        name: String,
        invested: f64,
        current_value: f64,
    }
    let month_clause = month
        .map(|v| format!(" AND h.month = {}", v))
        .unwrap_or_default();
    let year_clause = year
        .map(|v| format!(" AND h.year = {}", v))
        .unwrap_or_default();
    let (select, join, group) = if by_type {
        (
            "ht.name AS name",
            "JOIN holding_types ht ON ht.id = h.holding_type_id",
            "ht.name",
        )
    } else {
        ("h.platform AS name", "", "h.platform")
    };
    let rows = Row::find_by_statement(Statement::from_string(
        DbBackend::Postgres,
        format!(
            "SELECT {}, COALESCE(SUM(h.invested_amount), 0)::float8 AS invested, COALESCE(SUM(h.current_value), 0)::float8 AS current_value FROM holdings h {} WHERE h.user_id = '{}'{}{} GROUP BY {} ORDER BY {} ASC",
            select, join, user_id, month_clause, year_clause, group, group
        ),
    ))
    .all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| BreakdownValues {
            name: row.name,
            invested: row.invested,
            current: row.current_value,
        })
        .collect())
}

fn string_breakdown(data: &[BreakdownValues]) -> Vec<HoldingNamedStringBreakdown> {
    data.iter()
        .map(|item| {
            let profit_loss = item.current - item.invested;
            HoldingNamedStringBreakdown {
                name: item.name.clone(),
                invested: format_float(item.invested),
                current: format_float(item.current),
                profit_loss: format_float(profit_loss),
                profit_loss_percentage: format_float(calc_percent(item.invested, item.current)),
            }
        })
        .collect()
}

pub async fn summary(
    db: &DatabaseConnection,
    user_id: Uuid,
    month: Option<i32>,
    year: Option<i32>,
) -> Result<HoldingSummaryResponse, HoldingError> {
    let summary = summary_values(db, user_id, month, year).await?;
    let type_data = named_breakdown(db, user_id, month, year, true).await?;
    let platform_data = named_breakdown(db, user_id, month, year, false).await?;
    let profit_loss = summary.current - summary.invested;
    Ok(HoldingSummaryResponse {
        total_invested: format_float(summary.invested),
        total_current_value: format_float(summary.current),
        total_profit_loss: format_float(profit_loss),
        total_profit_loss_percentage: format_float(calc_percent(summary.invested, summary.current)),
        holdings_count: summary.count,
        type_breakdown: string_breakdown(&type_data),
        platform_breakdown: string_breakdown(&platform_data),
    })
}

pub async fn trends(
    db: &DatabaseConnection,
    user_id: Uuid,
    years: Vec<i32>,
) -> Result<Vec<HoldingTrendResponse>, HoldingError> {
    #[derive(FromQueryResult)]
    struct Row {
        month: i32,
        year: i32,
        invested: f64,
        current_value: f64,
    }
    let year_clause = if years.is_empty() {
        String::new()
    } else {
        format!(
            " AND year IN ({})",
            years
                .into_iter()
                .map(|year| year.to_string())
                .collect::<Vec<_>>()
                .join(",")
        )
    };
    let rows = Row::find_by_statement(Statement::from_string(
        DbBackend::Postgres,
        format!(
            "SELECT month, year, COALESCE(SUM(invested_amount), 0)::float8 AS invested, COALESCE(SUM(current_value), 0)::float8 AS current_value FROM holdings WHERE user_id = '{}'{} GROUP BY year, month ORDER BY year ASC, month ASC",
            user_id, year_clause
        ),
    ))
    .all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| {
            let profit_loss = row.current_value - row.invested;
            HoldingTrendResponse {
                date: format!("{:04}-{:02}", row.year, row.month),
                invested: format_float(row.invested),
                current: format_float(row.current_value),
                profit_loss: format_float(profit_loss),
                profit_loss_percentage: format_float(calc_percent(row.invested, row.current_value)),
            }
        })
        .collect())
}

fn summary_as_values(
    summary: &SummaryValues,
    types: &[BreakdownValues],
    platforms: &[BreakdownValues],
) -> HoldingSummaryValues {
    let to_breakdown = |data: &[BreakdownValues]| {
        data.iter()
            .map(|item| {
                let profit_loss = item.current - item.invested;
                HoldingNamedBreakdown {
                    name: item.name.clone(),
                    invested: item.invested,
                    current: item.current,
                    profit_loss,
                    profit_loss_percentage: calc_percent(item.invested, item.current),
                }
            })
            .collect()
    };
    HoldingSummaryValues {
        total_invested: summary.invested,
        total_current_value: summary.current,
        total_profit_loss: summary.current - summary.invested,
        total_profit_loss_percentage: calc_percent(summary.invested, summary.current),
        holdings_count: summary.count,
        type_breakdown: to_breakdown(types),
        platform_breakdown: to_breakdown(platforms),
    }
}

fn compare_breakdown(
    from_data: Vec<BreakdownValues>,
    to_data: Vec<BreakdownValues>,
) -> Vec<HoldingCompareBreakdown> {
    let from_map: HashMap<String, BreakdownValues> = from_data
        .into_iter()
        .map(|item| (item.name.clone(), item))
        .collect();
    let to_map: HashMap<String, BreakdownValues> = to_data
        .into_iter()
        .map(|item| (item.name.clone(), item))
        .collect();
    let names: HashSet<String> = from_map.keys().chain(to_map.keys()).cloned().collect();
    names
        .into_iter()
        .map(|name| {
            let from = from_map.get(&name);
            let to = to_map.get(&name);
            let from_invested = from.map(|v| v.invested).unwrap_or_default();
            let from_current = from.map(|v| v.current).unwrap_or_default();
            let to_invested = to.map(|v| v.invested).unwrap_or_default();
            let to_current = to.map(|v| v.current).unwrap_or_default();
            let from_profit = from_current - from_invested;
            let to_profit = to_current - to_invested;
            HoldingCompareBreakdown {
                name,
                from: HoldingBreakdownValues {
                    invested: from_invested,
                    current: from_current,
                    profit_loss: from_profit,
                    profit_loss_percentage: calc_percent(from_invested, from_current),
                },
                to: HoldingBreakdownValues {
                    invested: to_invested,
                    current: to_current,
                    profit_loss: to_profit,
                    profit_loss_percentage: calc_percent(to_invested, to_current),
                },
                invested_diff: to_invested - from_invested,
                current_value_diff: to_current - from_current,
                profit_loss_diff: to_profit - from_profit,
                invested_diff_percentage: calc_percent(from_invested, to_invested),
                current_value_diff_percentage: calc_percent(from_current, to_current),
            }
        })
        .collect()
}

pub async fn compare_months(
    db: &DatabaseConnection,
    user_id: Uuid,
    from_month: i32,
    from_year: i32,
    to_month: i32,
    to_year: i32,
) -> Result<HoldingMonthComparisonResponse, HoldingError> {
    let from_summary = summary_values(db, user_id, Some(from_month), Some(from_year)).await?;
    let to_summary = summary_values(db, user_id, Some(to_month), Some(to_year)).await?;
    let from_types = named_breakdown(db, user_id, Some(from_month), Some(from_year), true).await?;
    let to_types = named_breakdown(db, user_id, Some(to_month), Some(to_year), true).await?;
    let from_platforms =
        named_breakdown(db, user_id, Some(from_month), Some(from_year), false).await?;
    let to_platforms = named_breakdown(db, user_id, Some(to_month), Some(to_year), false).await?;
    let from_profit = from_summary.current - from_summary.invested;
    let to_profit = to_summary.current - to_summary.invested;

    Ok(HoldingMonthComparisonResponse {
        from_month: HoldingMonthPoint {
            month: from_month,
            year: from_year,
        },
        to_month: HoldingMonthPoint {
            month: to_month,
            year: to_year,
        },
        summary: HoldingCompareSummary {
            from: summary_as_values(&from_summary, &from_types, &from_platforms),
            to: summary_as_values(&to_summary, &to_types, &to_platforms),
            invested_diff: to_summary.invested - from_summary.invested,
            current_value_diff: to_summary.current - from_summary.current,
            profit_loss_diff: to_profit - from_profit,
            holdings_count_diff: to_summary.count - from_summary.count,
            invested_diff_percentage: calc_percent(from_summary.invested, to_summary.invested),
            current_value_diff_percentage: calc_percent(from_summary.current, to_summary.current),
            holdings_count_diff_percentage: calc_percent_i64(from_summary.count, to_summary.count),
        },
        type_comparison: compare_breakdown(from_types, to_types),
        platform_comparison: compare_breakdown(from_platforms, to_platforms),
    })
}

pub async fn monthly_data(
    db: &DatabaseConnection,
    user_id: Uuid,
    start_month: i32,
    start_year: i32,
    end_month: i32,
    end_year: i32,
) -> Result<Vec<HoldingMonthlyDataResponse>, HoldingError> {
    // The loop below walks forward from (end_month, end_year) to
    // (start_month, start_year), so End must be on or before Start
    // chronologically. Reject inverted ranges up front — without this guard
    // the loop never terminates.
    if end_year > start_year || (end_year == start_year && end_month > start_month) {
        return Err(HoldingError::InvalidRange);
    }

    #[derive(FromQueryResult)]
    struct Row {
        month: i32,
        year: i32,
        invested: f64,
        current_value: f64,
        count: i64,
    }
    let rows = Row::find_by_statement(Statement::from_string(
        DbBackend::Postgres,
        format!(
            "SELECT month, year, COALESCE(SUM(invested_amount), 0)::float8 AS invested, COALESCE(SUM(current_value), 0)::float8 AS current_value, COUNT(*)::bigint AS count FROM holdings WHERE user_id = '{}' AND ((year > {} OR (year = {} AND month >= {})) AND (year < {} OR (year = {} AND month <= {}))) GROUP BY year, month ORDER BY year ASC, month ASC",
            user_id, end_year, end_year, end_month, start_year, start_year, start_month
        ),
    ))
    .all(db)
    .await?;
    let map: HashMap<(i32, i32), Row> = rows
        .into_iter()
        .map(|row| ((row.year, row.month), row))
        .collect();
    let mut result = Vec::new();
    let mut month = end_month;
    let mut year = end_year;
    loop {
        let date = format!("{:04}-{:02}", year, month);
        if let Some(row) = map.get(&(year, month)) {
            result.push(HoldingMonthlyDataResponse {
                month: row.month,
                year: row.year,
                date,
                total_current_value: format_float(row.current_value),
                total_invested: format_float(row.invested),
                holdings_count: row.count,
            });
        } else {
            result.push(HoldingMonthlyDataResponse {
                month,
                year,
                date,
                total_current_value: "0".to_string(),
                total_invested: "0".to_string(),
                holdings_count: 0,
            });
        }
        if month == start_month && year == start_year {
            break;
        }
        month += 1;
        if month > 12 {
            month = 1;
            year += 1;
        }
    }
    Ok(result)
}

pub async fn duplicate_holdings(
    db: &DatabaseConnection,
    user_id: Uuid,
    from_month: i32,
    from_year: i32,
    to_month: i32,
    to_year: i32,
    overwrite: bool,
) -> Result<Vec<DuplicateResultItem>, HoldingError> {
    if from_month == to_month && from_year == to_year {
        return Err(HoldingError::DuplicateSameMonth);
    }
    let source = holdings::Entity::find()
        .filter(holdings::Column::UserId.eq(user_id))
        .filter(holdings::Column::Month.eq(from_month))
        .filter(holdings::Column::Year.eq(from_year))
        .all(db)
        .await?;
    if source.is_empty() {
        return Err(HoldingError::NotFound);
    }
    if overwrite {
        holdings::Entity::delete_many()
            .filter(holdings::Column::UserId.eq(user_id))
            .filter(holdings::Column::Month.eq(to_month))
            .filter(holdings::Column::Year.eq(to_year))
            .exec(db)
            .await?;
    }
    let mut out = Vec::with_capacity(source.len());
    for item in source {
        let now = Utc::now().into();
        let created = holdings::ActiveModel {
            user_id: Set(user_id),
            name: Set(item.name),
            symbol: Set(item.symbol),
            platform: Set(item.platform),
            holding_type_id: Set(item.holding_type_id),
            currency: Set(item.currency),
            invested_amount: Set(item.invested_amount),
            current_value: Set(item.current_value),
            units: Set(item.units),
            avg_buy_price: Set(item.avg_buy_price),
            current_price: Set(item.current_price),
            notes: Set(item.notes),
            month: Set(to_month),
            year: Set(to_year),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(db)
        .await?;
        out.push(DuplicateResultItem {
            id: created.id.to_string(),
            name: created.name,
            month: created.month,
            year: created.year,
        });
    }
    Ok(out)
}

pub fn default_current_month_year() -> (i32, i32) {
    let now = Utc::now();
    (now.month() as i32, now.year())
}

pub fn prev_month(month: i32, year: i32) -> (i32, i32) {
    if month == 1 {
        (12, year - 1)
    } else {
        (month - 1, year)
    }
}

pub fn prev_n_months(mut month: i32, mut year: i32, n: i32) -> (i32, i32) {
    for _ in 0..n {
        (month, year) = prev_month(month, year);
    }
    (month, year)
}

pub async fn sync_prices(_db: &DatabaseConnection, _user_id: Uuid) -> HoldingSyncResponse {
    let (month, year) = default_current_month_year();
    HoldingSyncResponse {
        synced_count: 0,
        month,
        year,
    }
}
