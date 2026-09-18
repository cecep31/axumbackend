use crate::dto::exchange_rate::ExchangeRateResponse;
use chrono::Utc;
use once_cell::sync::Lazy;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const YAHOO_SPARK_URL: &str = "https://query1.finance.yahoo.com/v7/finance/spark";
/// Mirrors echobackend's `exchangeRateCacheTTL` (`internal/service/exchange_rate_service.go`).
const CACHE_TTL: Duration = Duration::from_secs(15 * 60);

type ExchangeRateCacheMap = HashMap<(String, String), (Instant, ExchangeRateResponse)>;

static CACHE: Lazy<Mutex<ExchangeRateCacheMap>> = Lazy::new(|| Mutex::new(HashMap::new()));

#[derive(Debug)]
pub enum ExchangeRateError {
    InvalidCurrencyPair,
    Request(reqwest::Error),
    Upstream(String),
    NotFound(String, String),
}

impl From<reqwest::Error> for ExchangeRateError {
    fn from(err: reqwest::Error) -> Self {
        Self::Request(err)
    }
}

const MAX_EXCHANGE_RATE_CACHE_ENTRIES: usize = 200;

fn cache_get(from: &str, to: &str) -> Option<ExchangeRateResponse> {
    let mut cache = CACHE.lock().unwrap();
    let key = (from.to_string(), to.to_string());
    if let Some((stored_at, response)) = cache.get(&key) {
        if stored_at.elapsed() < CACHE_TTL {
            let mut response = response.clone();
            response.cached = true;
            return Some(response);
        }
        // Entry is expired; remove it to free heap memory
        cache.remove(&key);
    }
    None
}

fn cache_set(from: &str, to: &str, response: ExchangeRateResponse) {
    let mut cache = CACHE.lock().unwrap();

    // Prune all expired entries
    cache.retain(|_, (stored_at, _)| stored_at.elapsed() < CACHE_TTL);

    // Reclaim heap memory if capacity is disproportionately large
    if cache.capacity() > 64 && cache.len() * 4 < cache.capacity() {
        let min_cap = cache.len().max(16);
        cache.shrink_to(min_cap);
    }

    // Evict oldest entry if at capacity limit
    if cache.len() >= MAX_EXCHANGE_RATE_CACHE_ENTRIES
        && let Some(oldest_key) = cache
            .iter()
            .min_by_key(|(_, (stored_at, _))| *stored_at)
            .map(|(k, _)| k.clone())
    {
        cache.remove(&oldest_key);
    }

    cache.insert(
        (from.to_string(), to.to_string()),
        (Instant::now(), response),
    );
}

pub async fn get_rate(from: String, to: String) -> Result<ExchangeRateResponse, ExchangeRateError> {
    let from = normalize_currency_code(&from);
    let to = normalize_currency_code(&to);
    if !valid_currency_code(&from) || !valid_currency_code(&to) {
        return Err(ExchangeRateError::InvalidCurrencyPair);
    }

    if let Some(cached) = cache_get(&from, &to) {
        return Ok(cached);
    }

    if from == to {
        let result = response(&from, &to, &format!("{from}{to}=X"), 1.0);
        cache_set(&from, &to, result.clone());
        return Ok(result);
    }

    let direct_symbol = yahoo_currency_symbol(&from, &to);
    let inverse_symbol = yahoo_currency_symbol(&to, &from);
    let quotes = fetch_quotes(&[direct_symbol.clone(), inverse_symbol.clone()]).await?;

    if let Some(rate) = quotes
        .get(&direct_symbol)
        .copied()
        .filter(|rate| *rate > 0.0)
    {
        let result = response(&from, &to, &direct_symbol, rate);
        cache_set(&from, &to, result.clone());
        return Ok(result);
    }

    if let Some(inverse_rate) = quotes
        .get(&inverse_symbol)
        .copied()
        .filter(|rate| *rate > 0.0)
    {
        let rate = ((1.0 / inverse_rate) * 100_000_000.0).round() / 100_000_000.0;
        let result = response(&from, &to, &inverse_symbol, rate);
        cache_set(&from, &to, result.clone());
        return Ok(result);
    }

    Err(ExchangeRateError::NotFound(from, to))
}

fn response(from: &str, to: &str, symbol: &str, rate: f64) -> ExchangeRateResponse {
    ExchangeRateResponse {
        from: from.to_string(),
        to: to.to_string(),
        symbol: symbol.to_string(),
        rate,
        source: "Yahoo Finance".to_string(),
        cached: false,
        fetched_at: Utc::now().to_rfc3339(),
    }
}

async fn fetch_quotes(symbols: &[String]) -> Result<HashMap<String, f64>, ExchangeRateError> {
    if symbols.is_empty() {
        return Ok(HashMap::new());
    }

    let client = reqwest::Client::new();
    let payload = client
        .get(YAHOO_SPARK_URL)
        .query(&[
            ("symbols", symbols.join(",")),
            ("range", "1d".to_string()),
            ("interval", "1d".to_string()),
        ])
        .header(reqwest::header::USER_AGENT, "Mozilla/5.0")
        .send()
        .await?
        .error_for_status()?
        .json::<SparkResponse>()
        .await?;

    if let Some(error) = payload.spark.error {
        return Err(ExchangeRateError::Upstream(error.description));
    }

    let mut quotes = HashMap::new();
    for result in payload.spark.result {
        if let Some(response) = result.response.first()
            && response.meta.regular_market_price > 0.0
        {
            quotes.insert(
                result.symbol.trim().to_ascii_uppercase(),
                response.meta.regular_market_price,
            );
        }
    }

    Ok(quotes)
}

fn normalize_currency_code(code: &str) -> String {
    code.trim().to_ascii_uppercase()
}

fn valid_currency_code(code: &str) -> bool {
    code.len() == 3 && code.bytes().all(|b| b.is_ascii_uppercase())
}

fn yahoo_currency_symbol(from: &str, to: &str) -> String {
    format!("{from}{to}=X")
}

#[derive(Deserialize)]
struct SparkResponse {
    spark: Spark,
}

#[derive(Deserialize)]
struct Spark {
    result: Vec<SparkResult>,
    error: Option<SparkError>,
}

#[derive(Deserialize)]
struct SparkError {
    description: String,
}

#[derive(Deserialize)]
struct SparkResult {
    symbol: String,
    response: Vec<SparkResultResponse>,
}

#[derive(Deserialize)]
struct SparkResultResponse {
    meta: SparkMeta,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SparkMeta {
    regular_market_price: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_currency_code() {
        assert!(valid_currency_code("USD"));
        assert!(valid_currency_code("IDR"));
        assert!(!valid_currency_code("usd"));
        assert!(!valid_currency_code("US"));
        assert!(!valid_currency_code("USDT"));
        assert!(!valid_currency_code("123"));
    }

    #[test]
    fn test_normalize_currency_code() {
        assert_eq!(normalize_currency_code("  usd  "), "USD");
        assert_eq!(normalize_currency_code("idr"), "IDR");
    }

    #[test]
    fn test_cache_set_and_get() {
        let resp = ExchangeRateResponse {
            from: "USD".into(),
            to: "IDR".into(),
            symbol: "USDIDR=X".into(),
            rate: 16000.0,
            source: "Yahoo Finance".into(),
            cached: false,
            fetched_at: Utc::now().to_rfc3339(),
        };

        cache_set("USD", "IDR", resp);
        let cached = cache_get("USD", "IDR");
        assert!(cached.is_some());
        let cached = cached.unwrap();
        assert!(cached.cached);
        assert_eq!(cached.rate, 16000.0);
    }

    #[test]
    fn test_cache_expired_entry_removal() {
        let resp = ExchangeRateResponse {
            from: "EUR".into(),
            to: "GBP".into(),
            symbol: "EURGBP=X".into(),
            rate: 0.85,
            source: "Yahoo Finance".into(),
            cached: false,
            fetched_at: Utc::now().to_rfc3339(),
        };

        // Insert manually with expired timestamp
        {
            let mut cache = CACHE.lock().unwrap();
            cache.insert(
                ("EUR".into(), "GBP".into()),
                (Instant::now() - CACHE_TTL - Duration::from_secs(10), resp),
            );
        }

        // cache_get should return None and remove the expired key
        assert!(cache_get("EUR", "GBP").is_none());

        {
            let cache = CACHE.lock().unwrap();
            assert!(!cache.contains_key(&("EUR".into(), "GBP".into())));
        }
    }
}
