use crate::dto::holding::{
    CalendarQuery, CompareQuery, CreateHoldingRequest, DuplicateHoldingRequest, HoldingQuery,
    MonthlyQuery, SummaryQuery, TrendsQuery, UpdateHoldingRequest, parse_holding_id,
};
use crate::http::auth::AuthUser;
use crate::http::error::AppError;
use crate::http::extract::{PJson, VBind, VQuery, raw_path_param};
use crate::http::response::ApiResponse;
use crate::infra::database::DbPool;
use crate::models::corporate_action::CorporateActionCalendarResponse;
use crate::models::holding::{
    DuplicateResultItem, HoldingMonthComparisonResponse, HoldingMonthlyDataResponse,
    HoldingResponse, HoldingSummaryResponse, HoldingSyncResponse, HoldingTrendResponse,
    HoldingTypeResponse,
};
use crate::services::{self, holding::HoldingError};
use crate::state::AppState;
use axum::{
    Json, Router,
    extract::{FromRequestParts, State},
    http::request::Parts,
    routing::{get, post},
};
use chrono::{Datelike, Utc};

/// echobackend `respondHoldingError(c, message, err)`: the handler message
/// as `message`, the sentinel text as `error` (404/400), or a 500 with the
/// handler message and no `error`.
fn holding_err(message: &'static str) -> impl FnOnce(HoldingError) -> AppError {
    move |err| match err {
        HoldingError::Db(err) => AppError::from(err).context(message),
        HoldingError::NotFound => AppError::not_found(message, Some("holding not found".into())),
        HoldingError::HoldingTypeNotFound => {
            AppError::bad_request(message, Some("holding type not found".into()))
        }
        HoldingError::InvalidDecimal(field) => {
            AppError::bad_request(message, Some(format!("Invalid decimal value for {field}")))
        }
        HoldingError::DuplicateSameMonth => AppError::bad_request(
            message,
            Some("source and target month/year are the same".into()),
        ),
        HoldingError::InvalidRange => {
            AppError::bad_request(message, Some(INVALID_RANGE_ERROR.to_string()))
        }
        HoldingError::RangeTooLarge => AppError::bad_request(
            message,
            Some("requested month/year range is too large".to_string()),
        ),
        HoldingError::Quote(err) => AppError::internal(message, err),
    }
}

/// The `{id}` of `/api/holdings/{id}`: echobackend `strconv.ParseInt(.., 10,
/// 64)`, otherwise 400 "Invalid holding ID" with Go's parse error as `error`.
pub struct HoldingIdParam(pub i64);

impl<S: Send + Sync> FromRequestParts<S> for HoldingIdParam {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let raw = raw_path_param(parts, state, "id").await.unwrap_or_default();
        parse_holding_id(&raw)
            .map(HoldingIdParam)
            .map_err(|err| AppError::bad_request("Invalid holding ID", Some(err)))
    }
}

/// echobackend `ErrHoldingInvalidRange` text.
const INVALID_RANGE_ERROR: &str = "end month/year must be on or before start month/year";

fn parse_years(raw: Option<String>) -> Vec<i32> {
    raw.unwrap_or_default()
        .split(',')
        // echobackend `strconv.Atoi` on each part, untrimmed: "2024, 2025"
        // keeps only 2024.
        .filter_map(|value| value.parse::<i32>().ok())
        .collect()
}

pub async fn get_holdings(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VQuery(query): VQuery<HoldingQuery>,
) -> Result<Json<ApiResponse<Vec<HoldingResponse>>>, AppError> {
    let (current_month, current_year) = services::holding::default_current_month_year();
    let holdings = services::holding::get_holdings(
        &pool,
        auth_user.id,
        Some(query.month().unwrap_or(current_month)),
        Some(query.year().unwrap_or(current_year)),
        query.sort_by.as_deref(),
        query.order.as_deref(),
    )
    .await
    .map_err(holding_err("Failed to get holdings"))?;

    Ok(Json(ApiResponse::success_with_message(
        "Holdings fetched successfully",
        holdings,
    )))
}

pub async fn get_holding_by_id(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    HoldingIdParam(id): HoldingIdParam,
) -> Result<Json<ApiResponse<HoldingResponse>>, AppError> {
    let holding = services::holding::get_holding_by_id(&pool, id, auth_user.id)
        .await
        .map_err(holding_err("Failed to get holding"))?;
    Ok(Json(ApiResponse::success_with_message(
        "Holding fetched successfully",
        holding,
    )))
}

pub async fn create_holding(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    // echobackend `c.Bind` + `c.Validate`: "Invalid request format" on a bad
    // body, an empty body binds to zero values (422 `required`).
    VBind(req): VBind<CreateHoldingRequest>,
) -> Result<
    (
        axum::http::StatusCode,
        Json<ApiResponse<Vec<HoldingResponse>>>,
    ),
    AppError,
> {
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
    .map_err(holding_err("Failed to create holding"))?;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(ApiResponse::success_with_message(
            "Holding created successfully",
            vec![holding],
        )),
    ))
}

pub async fn update_holding(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    HoldingIdParam(id): HoldingIdParam,
    // Unvalidated, like echobackend's UpdateHolding (it binds the body but
    // never calls `c.Validate(req)`); a body that does not bind is a 400
    // "Invalid request format" envelope.
    PJson(req): PJson<UpdateHoldingRequest>,
) -> Result<Json<ApiResponse<Vec<HoldingResponse>>>, AppError> {
    let holding = services::holding::update_holding(
        &pool,
        id,
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
    .map_err(holding_err("Failed to update holding"))?;

    Ok(Json(ApiResponse::success_with_message(
        "Holding updated successfully",
        vec![holding],
    )))
}

pub async fn delete_holding(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    HoldingIdParam(id): HoldingIdParam,
) -> Result<Json<ApiResponse<()>>, AppError> {
    services::holding::delete_holding(&pool, id, auth_user.id)
        .await
        .map_err(holding_err("Failed to delete holding"))?;
    Ok(Json(ApiResponse::message("Holding deleted successfully")))
}

pub async fn get_holding_types(
    State(pool): State<DbPool>,
    _auth_user: AuthUser,
) -> Result<Json<ApiResponse<Vec<HoldingTypeResponse>>>, AppError> {
    let types = services::holding::get_holding_types(&pool)
        .await
        .map_err(holding_err("Failed to get holding types"))?;
    Ok(Json(ApiResponse::success_with_message(
        "Holding types fetched successfully",
        types,
    )))
}

pub async fn get_summary(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VQuery(query): VQuery<SummaryQuery>,
) -> Result<Json<ApiResponse<HoldingSummaryResponse>>, AppError> {
    let summary = services::holding::summary(&pool, auth_user.id, query.month(), query.year())
        .await
        .map_err(holding_err("Failed to get holdings summary"))?;
    Ok(Json(ApiResponse::success_with_message(
        "Holdings summary fetched successfully",
        summary,
    )))
}

pub async fn get_trends(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VQuery(query): VQuery<TrendsQuery>,
) -> Result<Json<ApiResponse<Vec<HoldingTrendResponse>>>, AppError> {
    let trends = services::holding::trends(&pool, auth_user.id, parse_years(query.years.clone()))
        .await
        .map_err(holding_err("Failed to get holdings trends"))?;
    Ok(Json(ApiResponse::success_with_message(
        "Holdings trends fetched successfully",
        trends,
    )))
}

pub async fn compare_months(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VQuery(query): VQuery<CompareQuery>,
) -> Result<Json<ApiResponse<HoldingMonthComparisonResponse>>, AppError> {
    let (current_month, current_year) = services::holding::default_current_month_year();
    let to_month = query.to_month().unwrap_or(current_month);
    let to_year = query.to_year().unwrap_or(current_year);

    // Mirrors echobackend's CompareMonths query parsing: only derive From via
    // prevMonth(To) when *both* fromMonth and fromYear are omitted; if only
    // one is missing, it's filled from To's own value.
    let (from_month, from_year) = match (query.from_month(), query.from_year()) {
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
    .map_err(holding_err("Failed to compare months"))?;
    Ok(Json(ApiResponse::success_with_message(
        "Month comparison fetched successfully",
        result,
    )))
}

pub async fn get_monthly_data(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VQuery(query): VQuery<MonthlyQuery>,
) -> Result<Json<ApiResponse<Vec<HoldingMonthlyDataResponse>>>, AppError> {
    let (current_month, current_year) = services::holding::default_current_month_year();
    // echobackend `parseMonthlyQuery`: a present year must be an integer in
    // [1990, current year + 1], otherwise the range is rejected (months stay
    // lenient).
    let start_year = query
        .start_year_bounded()
        .map_err(holding_err("Invalid monthly range"))?;
    let query_end_year = query
        .end_year_bounded()
        .map_err(holding_err("Invalid monthly range"))?;
    let start_month = query.start_month().unwrap_or(current_month);
    let start_year = start_year.unwrap_or(current_year);

    // Mirrors echobackend's parseMonthlyQuery: only derive End via
    // prevNMonths(Start, 11) when *both* endMonth and endYear are omitted;
    // if only one is missing, it's filled from Start's own value.
    let (mut end_month, mut end_year) = match (query.end_month(), query_end_year) {
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
    .map_err(holding_err("Failed to get monthly data"))?;
    Ok(Json(ApiResponse::success_with_message(
        "Holdings monthly data fetched successfully",
        result,
    )))
}

pub async fn sync_prices(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
) -> Result<Json<ApiResponse<HoldingSyncResponse>>, AppError> {
    let result = services::holding::sync_prices(&pool, auth_user.id)
        .await
        .map_err(holding_err("Failed to sync prices"))?;
    Ok(Json(ApiResponse::success_with_message(
        "Prices synced successfully for current month",
        result,
    )))
}

pub async fn duplicate_holdings(
    State(pool): State<DbPool>,
    auth_user: AuthUser,
    VBind(req): VBind<DuplicateHoldingRequest>,
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
    .map_err(holding_err("Failed to duplicate holdings"))?;

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
    State(pool): State<DbPool>,
    _auth_user: AuthUser,
    VQuery(query): VQuery<CalendarQuery>,
) -> Result<Json<ApiResponse<CorporateActionCalendarResponse>>, AppError> {
    let now = Utc::now();
    let month = query.month().unwrap_or(now.month() as i32);
    let year = query.year().unwrap_or(now.year());

    let result = services::corporate_action::get_calendar(&pool, year, month).await;
    Ok(Json(ApiResponse::success_with_message(
        "Corporate actions calendar fetched successfully",
        result,
    )))
}

pub fn routes() -> Router<AppState> {
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
