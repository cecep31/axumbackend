use super::{
    BreakdownValues, HoldingError, SummaryValues, calc_percent, calc_percent_i64, format_float,
};
use crate::models::holding::*;
use chrono::{Datelike, Utc};
use sea_orm::{DatabaseConnection, DbBackend, DbErr, FromQueryResult, Statement};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

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
