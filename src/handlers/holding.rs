use crate::auth::AuthUser;
use crate::database::DbPool;
use crate::dto::holding::{
    CalendarQuery, CompareQuery, CreateHoldingRequest, DuplicateHoldingRequest, HoldingPath,
    HoldingQuery, MonthlyQuery, SummaryQuery, TrendsQuery, UpdateHoldingRequest,
};
use crate::error::AppError;
use crate::models::corporate_action::CorporateActionCalendarResponse;
use crate::models::holding::{
    DuplicateResultItem, HoldingMonthComparisonResponse, HoldingMonthlyDataResponse,
    HoldingResponse, HoldingSummaryResponse, HoldingSyncResponse, HoldingTrendResponse,
    HoldingTypeResponse,
};
use crate::response::ApiResponse;
use crate::services::{self, holding::HoldingError};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
use axum_valid::Valid;
use chrono::{Datelike, NaiveDate, Utc};

fn map_holding_error(err: HoldingError) -> AppError {
    match err {
        HoldingError::Db(err) => AppError::from(err),
        HoldingError::NotFound => AppError::NotFound("Holding not found".to_string()),
        HoldingError::HoldingTypeNotFound => {
            AppError::BadRequest("Holding type not found".to_string())
        }
        HoldingError::InvalidDecimal(field) => {
            AppError::BadRequest(format!("Invalid decimal value for {}", field))
        }
        HoldingError::DuplicateSameMonth => {
            AppError::BadRequest("Cannot duplicate holdings into the same month".to_string())
        }
        HoldingError::InvalidRange => AppError::BadRequest("Invalid monthly range".to_string()),
    }
}

fn parse_years(raw: Option<String>) -> Vec<i32> {
    raw.unwrap_or_default()
        .split(',')
        .filter_map(|value| value.trim().parse::<i32>().ok())
        .collect()
}

pub async fn get_holdings(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    Valid(query): Valid<Query<HoldingQuery>>,
) -> Result<Json<ApiResponse<Vec<HoldingResponse>>>, AppError> {
    let (current_month, current_year) = services::holding::default_current_month_year();
    let holdings = services::holding::get_holdings(
        &pool,
        auth_user.id,
        Some(query.month.unwrap_or(current_month)),
        Some(query.year.unwrap_or(current_year)),
        query.sort_by.as_deref(),
        query.order.as_deref(),
    )
    .await
    .map_err(map_holding_error)?;

    Ok(Json(ApiResponse::success_with_message(
        "Holdings fetched successfully",
        holdings,
    )))
}

pub async fn get_holding_by_id(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    Valid(Path(params)): Valid<Path<HoldingPath>>,
) -> Result<Json<ApiResponse<HoldingResponse>>, AppError> {
    let holding = services::holding::get_holding_by_id(&pool, params.id, auth_user.id)
        .await
        .map_err(map_holding_error)?;
    Ok(Json(ApiResponse::success_with_message(
        "Holding fetched successfully",
        holding,
    )))
}

pub async fn create_holding(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    Valid(Json(req)): Valid<Json<CreateHoldingRequest>>,
) -> Result<(axum::http::StatusCode, Json<ApiResponse<HoldingResponse>>), AppError> {
    let holding = services::holding::create_holding(
        &pool,
        auth_user.id,
        services::holding::CreateHoldingInput {
            name: req.name,
            symbol: req.symbol,
            platform: req.platform,
            holding_type_id: req.holding_type_id,
            currency: req.currency,
            invested_amount: req.invested_amount,
            current_value: req.current_value,
            units: req.units,
            avg_buy_price: req.avg_buy_price,
            current_price: req.current_price,
            last_updated: req.last_updated,
            notes: req.notes,
            month: req.month,
            year: req.year,
        },
    )
    .await
    .map_err(map_holding_error)?;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(ApiResponse::success_with_message(
            "Holding created successfully",
            holding,
        )),
    ))
}

pub async fn update_holding(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    Valid(Path(params)): Valid<Path<HoldingPath>>,
    Valid(Json(req)): Valid<Json<UpdateHoldingRequest>>,
) -> Result<Json<ApiResponse<HoldingResponse>>, AppError> {
    let holding = services::holding::update_holding(
        &pool,
        params.id,
        auth_user.id,
        services::holding::UpdateHoldingInput {
            name: req.name,
            symbol: req.symbol,
            platform: req.platform,
            holding_type_id: req.holding_type_id,
            currency: req.currency,
            invested_amount: req.invested_amount,
            current_value: req.current_value,
            units: req.units,
            avg_buy_price: req.avg_buy_price,
            current_price: req.current_price,
            last_updated: req.last_updated,
            notes: req.notes,
            month: req.month,
            year: req.year,
        },
    )
    .await
    .map_err(map_holding_error)?;

    Ok(Json(ApiResponse::success_with_message(
        "Holding updated successfully",
        holding,
    )))
}

pub async fn delete_holding(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    Valid(Path(params)): Valid<Path<HoldingPath>>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    services::holding::delete_holding(&pool, params.id, auth_user.id)
        .await
        .map_err(map_holding_error)?;
    Ok(Json(ApiResponse::success_with_message(
        "Holding deleted successfully",
        serde_json::Value::Null,
    )))
}

pub async fn get_holding_types(
    State(pool): State<DbPool>,
    _auth_user: AuthUser,
) -> Result<Json<ApiResponse<Vec<HoldingTypeResponse>>>, AppError> {
    let types = services::holding::get_holding_types(&pool)
        .await
        .map_err(map_holding_error)?;
    Ok(Json(ApiResponse::success_with_message(
        "Holding types fetched successfully",
        types,
    )))
}

pub async fn get_summary(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    Valid(query): Valid<Query<SummaryQuery>>,
) -> Result<Json<ApiResponse<HoldingSummaryResponse>>, AppError> {
    let summary = services::holding::summary(&pool, auth_user.id, query.month, query.year)
        .await
        .map_err(map_holding_error)?;
    Ok(Json(ApiResponse::success_with_message(
        "Holdings summary fetched successfully",
        summary,
    )))
}

pub async fn get_trends(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    Valid(query): Valid<Query<TrendsQuery>>,
) -> Result<Json<ApiResponse<Vec<HoldingTrendResponse>>>, AppError> {
    let trends = services::holding::trends(&pool, auth_user.id, parse_years(query.years.clone()))
        .await
        .map_err(map_holding_error)?;
    Ok(Json(ApiResponse::success_with_message(
        "Holdings trends fetched successfully",
        trends,
    )))
}

pub async fn compare_months(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    Valid(query): Valid<Query<CompareQuery>>,
) -> Result<Json<ApiResponse<HoldingMonthComparisonResponse>>, AppError> {
    let (current_month, current_year) = services::holding::default_current_month_year();
    let to_month = query.to_month.unwrap_or(current_month);
    let to_year = query.to_year.unwrap_or(current_year);

    // Mirrors echobackend's CompareMonths query parsing: only derive From via
    // prevMonth(To) when *both* fromMonth and fromYear are omitted; if only
    // one is missing, it's filled from To's own value.
    let (from_month, from_year) = match (query.from_month, query.from_year) {
        (None, None) => services::holding::prev_month(to_month, to_year),
        (None, Some(from_year)) => (to_month, from_year),
        (Some(from_month), None) => (from_month, to_year),
        (Some(from_month), Some(from_year)) => (from_month, from_year),
    };

    let result = services::holding::compare_months(
        &pool,
        auth_user.id,
        from_month,
        from_year,
        to_month,
        to_year,
    )
    .await
    .map_err(map_holding_error)?;
    Ok(Json(ApiResponse::success_with_message(
        "Month comparison fetched successfully",
        result,
    )))
}

pub async fn get_monthly_data(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    Valid(query): Valid<Query<MonthlyQuery>>,
) -> Result<Json<ApiResponse<Vec<HoldingMonthlyDataResponse>>>, AppError> {
    let (current_month, current_year) = services::holding::default_current_month_year();
    let start_month = query.start_month.unwrap_or(current_month);
    let start_year = query.start_year.unwrap_or(current_year);

    // Mirrors echobackend's parseMonthlyQuery: only derive End via
    // prevNMonths(Start, 11) when *both* endMonth and endYear are omitted;
    // if only one is missing, it's filled from Start's own value.
    let (mut end_month, mut end_year) = match (query.end_month, query.end_year) {
        (None, None) => services::holding::prev_n_months(start_month, start_year, 11),
        (None, Some(end_year)) => (start_month, end_year),
        (Some(end_month), None) => (end_month, start_year),
        (Some(end_month), Some(end_year)) => (end_month, end_year),
    };
    let mut start_month = start_month;
    let mut start_year = start_year;

    // The service expects Start to be the chronologically latest month and
    // End the oldest — normalize so callers can pass them in any order
    // (also guards against the forward-iterating loop never terminating).
    if end_year > start_year || (end_year == start_year && end_month > start_month) {
        std::mem::swap(&mut start_month, &mut end_month);
        std::mem::swap(&mut start_year, &mut end_year);
    }

    let result = services::holding::monthly_data(
        &pool,
        auth_user.id,
        start_month,
        start_year,
        end_month,
        end_year,
    )
    .await
    .map_err(map_holding_error)?;
    Ok(Json(ApiResponse::success_with_message(
        "Holdings monthly data fetched successfully",
        result,
    )))
}

pub async fn sync_prices(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
) -> Result<Json<ApiResponse<HoldingSyncResponse>>, AppError> {
    let result = services::holding::sync_prices(&pool, auth_user.id).await;
    Ok(Json(ApiResponse::success_with_message(
        "Prices synced successfully for current month",
        result,
    )))
}

pub async fn duplicate_holdings(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    Valid(Json(req)): Valid<Json<DuplicateHoldingRequest>>,
) -> Result<
    (
        axum::http::StatusCode,
        Json<ApiResponse<Vec<DuplicateResultItem>>>,
    ),
    AppError,
> {
    let result = services::holding::duplicate_holdings(
        &pool,
        auth_user.id,
        req.from_month,
        req.from_year,
        req.to_month,
        req.to_year,
        req.overwrite,
    )
    .await
    .map_err(map_holding_error)?;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(ApiResponse::success_with_message(
            "Holdings duplicated successfully",
            result,
        )),
    ))
}

/// `GET /api/holdings/calendar` — corporate-actions calendar (dividends + RUPS)
/// for the authenticated user. Mirrors echobackend's `CorporateActionHandler.GetCalendar`.
pub async fn get_calendar(
    _auth_user: AuthUser,
    Valid(Query(query)): Valid<Query<CalendarQuery>>,
) -> Result<Json<ApiResponse<CorporateActionCalendarResponse>>, AppError> {
    let now = Utc::now();
    let from = query
        .from
        .as_deref()
        .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
        .unwrap_or_else(|| {
            NaiveDate::from_ymd_opt(now.year(), now.month(), 1)
                .expect("first day of current month is always valid")
        });
    let to = query
        .to
        .as_deref()
        .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
        .unwrap_or_else(|| services::corporate_action::add_months(now.date_naive(), 3));

    let result = services::corporate_action::get_calendar(from, to).await;
    Ok(Json(ApiResponse::success_with_message(
        "Corporate actions calendar fetched successfully",
        result,
    )))
}

pub fn routes() -> Router<DbPool> {
    Router::new()
        .route("/api/holdings", get(get_holdings).post(create_holding))
        .route("/api/holdings/summary", get(get_summary))
        .route("/api/holdings/trends", get(get_trends))
        .route("/api/holdings/compare", get(compare_months))
        .route("/api/holdings/monthly", get(get_monthly_data))
        .route("/api/holdings/calendar", get(get_calendar))
        .route("/api/holdings/duplicate", post(duplicate_holdings))
        .route("/api/holdings/sync", post(sync_prices))
        .route(
            "/api/holdings/{id}",
            get(get_holding_by_id)
                .put(update_holding)
                .delete(delete_holding),
        )
        .route("/api/holding-types", get(get_holding_types))
}
