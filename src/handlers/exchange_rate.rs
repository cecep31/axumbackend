use crate::auth::AuthUser;
use crate::database::DbPool;
use crate::dto::exchange_rate::ExchangeRateResponse;
use crate::error::AppError;
use crate::extract::VQuery;
use crate::response::ApiResponse;
use crate::services::exchange_rate::{self, ExchangeRateError};
use axum::{Json, Router, extract::State, routing::get};
use serde::Deserialize;
use validator::Validate;

#[derive(Debug, Default, Deserialize, Validate)]
pub struct ExchangeRateQuery {
    #[serde(default)]
    pub from: String,
    #[serde(default)]
    pub to: String,
}

fn map_exchange_rate_error(err: ExchangeRateError) -> AppError {
    match err {
        ExchangeRateError::InvalidCurrencyPair => {
            AppError::BadRequest("Invalid currency pair".to_string())
        }
        ExchangeRateError::NotFound(from, to) => {
            // Mirrors echobackend: a missing pair is a plain service error, not
            // `ErrInvalidCurrencyPair`, so it falls through to a generic 500
            // rather than a 404.
            AppError::InternalServerError(format!("Exchange rate not found for {from}/{to}"))
        }
        ExchangeRateError::Request(err) => {
            AppError::InternalServerError(format!("Failed to request exchange rate: {err}"))
        }
        ExchangeRateError::Upstream(message) => {
            AppError::InternalServerError(format!("Exchange rate upstream error: {message}"))
        }
    }
}

pub async fn get_rate(
    State(_pool): State<DbPool>,
    _auth_user: AuthUser,
    VQuery(query): VQuery<ExchangeRateQuery>,
) -> Result<Json<ApiResponse<ExchangeRateResponse>>, AppError> {
    let result = exchange_rate::get_rate(query.from, query.to)
        .await
        .map_err(map_exchange_rate_error)?;

    Ok(Json(ApiResponse::success_with_message(
        "Exchange rate fetched successfully",
        result,
    )))
}

pub fn routes() -> Router<DbPool> {
    Router::new().route("/api/exchange-rates", get(get_rate))
}
