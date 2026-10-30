use serde::{Deserialize, Serialize};

/// Also the cached JSON shape under `exchange-rate:<FROM>:<TO>` (shared with
/// echobackend's `dto.ExchangeRateResponse`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangeRateResponse {
    pub from: String,
    pub to: String,
    pub symbol: String,
    pub rate: f64,
    pub source: String,
    pub cached: bool,
    #[serde(rename = "fetchedAt")]
    pub fetched_at: String,
}
