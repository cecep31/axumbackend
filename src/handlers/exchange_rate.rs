use crate::http::auth::AuthUser;
use crate::http::error::AppError;
use crate::http::extract::VQuery;
use crate::http::response::ApiResponse;
use crate::infra::database::DbPool;
use crate::models::exchange_rate::ExchangeRateResponse;
use crate::services::exchange_rate::{self, ExchangeRateError};
use crate::state::AppState;
use axum::{Json, Router, extract::State, routing::get};
use garde::Validate;
use serde::Deserialize;

#[derive(Debug, Default, Deserialize, Validate)]
pub struct ExchangeRateQuery {
    #[serde(default)]
    #[garde(skip)]
    pub from: String,
    #[serde(default)]
    #[garde(skip)]
    pub to: String,
}

impl From<ExchangeRateError> for AppError {
    fn from(err: ExchangeRateError) -> Self {
        match err {
            ExchangeRateError::InvalidCurrencyPair => AppError::bad_request(
                "Invalid currency pair",
                Some("invalid currency pair".into()),
            ),
            // Mirrors echobackend: a missing pair is a plain service error, so it
            // is a generic 500 ("Failed to get exchange rate", no `error` key).
            ExchangeRateError::NotFound(from, to) => AppError::internal(
                "Failed to get exchange rate",
                format!("exchange rate not found for {from}/{to}"),
            ),
            ExchangeRateError::Upstream(message) => {
                AppError::internal("Failed to get exchange rate", message)
            }
        }
    }
}

pub async fn get_rate(
    State(_pool): State<DbPool>,
    _auth_user: AuthUser,
    VQuery(query): VQuery<ExchangeRateQuery>,
) -> Result<Json<ApiResponse<ExchangeRateResponse>>, AppError> {
    let result = exchange_rate::get_rate(query.from, query.to).await?;

    Ok(Json(ApiResponse::success_with_message(
        "Exchange rate fetched successfully",
        result,
    )))
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/exchange-rates", get(get_rate))
}
