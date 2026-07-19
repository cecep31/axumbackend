use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
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
