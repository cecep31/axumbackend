//! Exchange rates, mirroring echobackend's `exchangeRateService`: quotes
//! come from the shared RapidAPI quote client (`quote:<SYMBOL>` cache) and the
//! answer is cached in Redis under `<prefix>:exchange-rate:<FROM>:<TO>` for
//! 15 minutes (a hit is returned with `cached: true`). Without Redis every
//! call is answered fresh, like echobackend.

use crate::clients::market_quote;
use crate::config::MarketConfig;
use crate::infra::cache;
use crate::models::exchange_rate::ExchangeRateResponse;
use chrono::{SecondsFormat, Utc};
use std::time::Duration;

/// Mirrors echobackend's `exchangeRateCacheTTL`.
const CACHE_TTL: Duration = Duration::from_secs(15 * 60);

#[derive(Debug)]
pub enum ExchangeRateError {
    InvalidCurrencyPair,
    Upstream(String),
    NotFound(String, String),
}

fn cache_key<'a>(from: &'a str, to: &'a str) -> [&'a str; 3] {
    ["exchange-rate", from, to]
}

async fn cache_set(response: &ExchangeRateResponse) {
    cache::write_json(
        &cache_key(&response.from, &response.to),
        response,
        CACHE_TTL,
    )
    .await;
}

pub async fn get_rate(from: String, to: String) -> Result<ExchangeRateResponse, ExchangeRateError> {
    let from = normalize_currency_code(&from);
    let to = normalize_currency_code(&to);
    if !valid_currency_code(&from) || !valid_currency_code(&to) {
        return Err(ExchangeRateError::InvalidCurrencyPair);
    }

    if let Some(mut cached) = cache::read_json::<ExchangeRateResponse>(&cache_key(&from, &to)).await
    {
        cached.cached = true;
        return Ok(cached);
    }

    if from == to {
        let result = response(&from, &to, &format!("{from}{to}=X"), 1.0);
        cache_set(&result).await;
        return Ok(result);
    }

    let direct_symbol = yahoo_currency_symbol(&from, &to);
    let inverse_symbol = yahoo_currency_symbol(&to, &from);
    let api_key = MarketConfig::try_get()
        .map(|cfg| cfg.rapidapi_key.as_str())
        .unwrap_or_default();
    let quotes =
        market_quote::get_quotes(api_key, &[direct_symbol.clone(), inverse_symbol.clone()])
            .await
            .map_err(ExchangeRateError::Upstream)?;

    if let Some(rate) = quotes
        .get(&direct_symbol)
        .copied()
        .filter(|rate| *rate > 0.0)
    {
        let result = response(&from, &to, &direct_symbol, rate);
        cache_set(&result).await;
        return Ok(result);
    }

    if let Some(inverse_rate) = quotes
        .get(&inverse_symbol)
        .copied()
        .filter(|rate| *rate > 0.0)
    {
        let rate = ((1.0 / inverse_rate) * 100_000_000.0).round() / 100_000_000.0;
        let result = response(&from, &to, &inverse_symbol, rate);
        cache_set(&result).await;
        return Ok(result);
    }

    Err(ExchangeRateError::NotFound(from, to))
}

/// echobackend `response`: `source` "RapidAPI", `fetchedAt` in Go RFC3339
/// (UTC, `Z`, whole seconds).
fn response(from: &str, to: &str, symbol: &str, rate: f64) -> ExchangeRateResponse {
    ExchangeRateResponse {
        from: from.to_string(),
        to: to.to_string(),
        symbol: symbol.to_string(),
        rate,
        source: "RapidAPI".to_string(),
        cached: false,
        fetched_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
    }
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
    fn test_response_matches_echobackend() {
        let res = response("USD", "IDR", "USDIDR=X", 16000.0);
        assert_eq!(res.source, "RapidAPI");
        assert!(!res.cached);
        // Go time.RFC3339 in UTC: 2006-01-02T15:04:05Z
        assert_eq!(res.fetched_at.len(), 20);
        assert!(res.fetched_at.ends_with('Z'));
        assert_eq!(cache_key("USD", "IDR"), ["exchange-rate", "USD", "IDR"]);
    }
}
