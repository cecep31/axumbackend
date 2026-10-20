//! Market quote client (YH Finance via RapidAPI) with a Redis caching layer.
//!
//! Ports echobackend's `market.RapidAPIQuoteClient` wrapped in
//! `market.CachedQuoteClient`: symbols are normalized, cache hits are served
//! from Redis (`quote:<SYMBOL>`, 15 minute TTL), and misses are fetched in
//! batches of up to 50 symbols.

use crate::infra::cache;
use reqwest::Client;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;
use std::time::Duration;

const RAPIDAPI_QUOTE_HOST: &str = "yh-finance.p.rapidapi.com";
const RAPIDAPI_QUOTE_BASE_URL: &str = "https://yh-finance.p.rapidapi.com";
const MAX_SYMBOLS_PER_QUOTE_REQUEST: usize = 50;
const QUOTE_CACHE_TTL: Duration = Duration::from_secs(15 * 60);

fn shared_client() -> &'static Client {
    static CLIENT: LazyLock<Client> = LazyLock::new(|| {
        Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .unwrap_or_else(|_| Client::new())
    });
    &CLIENT
}

/// Trims, uppercases, and deduplicates ticker symbols, keeping first-seen order.
pub fn normalize_symbols<S: AsRef<str>>(symbols: &[S]) -> Vec<String> {
    let mut seen = HashSet::with_capacity(symbols.len());
    let mut normalized = Vec::with_capacity(symbols.len());
    for symbol in symbols {
        let symbol = symbol.as_ref().trim().to_uppercase();
        if symbol.is_empty() || !seen.insert(symbol.clone()) {
            continue;
        }
        normalized.push(symbol);
    }
    normalized
}

/// Fetches prices for `symbols`, serving cache hits from Redis and fetching
/// the misses from RapidAPI. Mirrors `CachedQuoteClient.GetQuotes`: if the
/// upstream call fails but some prices came from the cache, those partial
/// results are returned instead of the error.
pub async fn get_quotes<S: AsRef<str>>(
    api_key: &str,
    symbols: &[S],
) -> Result<HashMap<String, f64>, String> {
    let normalized = normalize_symbols(symbols);
    if normalized.is_empty() {
        return Ok(HashMap::new());
    }

    let Some(cache) = cache::get() else {
        return fetch_quotes(api_key, &normalized).await;
    };

    let mut results = HashMap::with_capacity(normalized.len());
    let mut missing = Vec::with_capacity(normalized.len());
    for symbol in normalized {
        let key = cache.build_key(&["quote", &symbol]);
        match cache.get_json::<f64>(&key).await {
            Ok(Some(price)) if price > 0.0 => {
                results.insert(symbol, price);
            }
            _ => missing.push(symbol),
        }
    }

    if missing.is_empty() {
        return Ok(results);
    }

    let fetched = match fetch_quotes(api_key, &missing).await {
        Ok(fetched) => fetched,
        Err(_) if !results.is_empty() => return Ok(results),
        Err(err) => return Err(err),
    };

    for (symbol, price) in fetched {
        if price <= 0.0 {
            continue;
        }
        let key = cache.build_key(&["quote", &symbol]);
        if let Err(err) = cache.set_json_with_ttl(&key, &price, QUOTE_CACHE_TTL).await {
            tracing::warn!(key, error = %err, "cache: failed to store quote");
        }
        results.insert(symbol, price);
    }

    Ok(results)
}

/// Fetches already-normalized symbols from RapidAPI in batches.
async fn fetch_quotes(api_key: &str, symbols: &[String]) -> Result<HashMap<String, f64>, String> {
    let mut quotes = HashMap::with_capacity(symbols.len());
    for batch in symbols.chunks(MAX_SYMBOLS_PER_QUOTE_REQUEST) {
        quotes.extend(fetch_batch(shared_client(), api_key, batch).await?);
    }
    Ok(quotes)
}

async fn fetch_batch(
    client: &Client,
    api_key: &str,
    symbols: &[String],
) -> Result<HashMap<String, f64>, String> {
    let joined = symbols.join(",");
    let mut request = client
        .get(format!("{RAPIDAPI_QUOTE_BASE_URL}/market/v2/get-quotes"))
        .query(&[("symbols", joined.as_str()), ("ticker", joined.as_str())])
        .header("X-RapidAPI-Host", RAPIDAPI_QUOTE_HOST)
        .header("Accept", "application/json")
        .header("User-Agent", "Mozilla/5.0 (pilput-backend)");
    if !api_key.is_empty() {
        request = request.header("X-RapidAPI-Key", api_key);
    }

    let response = request
        .send()
        .await
        .map_err(|err| format!("request rapidapi quotes: {err}"))?;
    let status = response.status();
    let body = response
        .bytes()
        .await
        .map_err(|err| format!("read rapidapi quote response: {err}"))?;
    if !status.is_success() {
        let lossy = String::from_utf8_lossy(&body);
        return Err(format!(
            "rapidapi quote returned status {}: {}",
            status.as_u16(),
            truncate(&lossy, 200)
        ));
    }

    Ok(parse_quote_response(&body))
}

/// Handles the JSON envelopes commonly used by RapidAPI quote providers:
/// 1. `{"body": [{"symbol": "AAPL", "regularMarketPrice": 150.0}, ...]}`
/// 2. `{"quoteResponse": {"result": [...]}}`
/// 3. `[{"symbol": "AAPL", "regularMarketPrice": 150.0}, ...]`
/// 4. `{"spark": {"result": [{"symbol": "AAPL", "response": [{"meta": {"regularMarketPrice": 150.0}}]}]}}`
fn parse_quote_response(body: &[u8]) -> HashMap<String, f64> {
    let mut quotes = HashMap::new();
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return quotes;
    };

    let items = non_empty(value.get("body"))
        .or_else(|| non_empty(value.pointer("/quoteResponse/result")))
        .or_else(|| non_empty(Some(&value)));
    if let Some(items) = items {
        for item in items {
            extract_quote_item(item, &mut quotes);
        }
        return quotes;
    }

    if let Some(results) = non_empty(value.pointer("/spark/result")) {
        for result in results {
            let symbol = result
                .get("symbol")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_uppercase();
            let price = result
                .pointer("/response/0/meta/regularMarketPrice")
                .and_then(Value::as_f64)
                .unwrap_or_default();
            if price > 0.0 {
                quotes.insert(symbol, price);
            }
        }
    }

    quotes
}

fn non_empty(value: Option<&Value>) -> Option<&Vec<Value>> {
    value
        .and_then(Value::as_array)
        .filter(|items| !items.is_empty())
}

fn extract_quote_item(item: &Value, quotes: &mut HashMap<String, f64>) {
    let symbol = item
        .get("symbol")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_uppercase();
    if symbol.is_empty() {
        return;
    }

    for field in ["regularMarketPrice", "price", "close"] {
        if let Some(price) = item.get(field).and_then(parse_raw_price)
            && price > 0.0
        {
            quotes.insert(symbol, price);
            return;
        }
    }
}

/// Accepts a number, a numeric string, or a `{"raw": 150.0, "fmt": "150.00"}` object.
fn parse_raw_price(raw: &Value) -> Option<f64> {
    match raw {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        Value::Object(obj) => obj
            .get("raw")
            .and_then(Value::as_f64)
            .filter(|raw| *raw > 0.0),
        _ => None,
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(n).collect();
        format!("{truncated}...")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_and_dedupes_symbols() {
        assert_eq!(
            normalize_symbols(&[" bbca.jk", "BBCA.JK", "", "aapl"]),
            vec!["BBCA.JK".to_string(), "AAPL".to_string()]
        );
    }

    #[test]
    fn parses_body_envelope() {
        let quotes = parse_quote_response(
            br#"{"body":[{"symbol":"aapl","regularMarketPrice":150.5},{"symbol":"MSFT","price":"300"}]}"#,
        );
        assert_eq!(quotes.get("AAPL"), Some(&150.5));
        assert_eq!(quotes.get("MSFT"), Some(&300.0));
    }

    #[test]
    fn parses_quote_response_envelope_with_raw_objects() {
        let quotes = parse_quote_response(
            br#"{"quoteResponse":{"result":[{"symbol":"BBCA.JK","regularMarketPrice":{"raw":9875.0,"fmt":"9,875"}}]}}"#,
        );
        assert_eq!(quotes.get("BBCA.JK"), Some(&9875.0));
    }

    #[test]
    fn parses_direct_slice_and_falls_back_to_close() {
        let quotes =
            parse_quote_response(br#"[{"symbol":"TLKM.JK","regularMarketPrice":0,"close":3200}]"#);
        assert_eq!(quotes.get("TLKM.JK"), Some(&3200.0));
    }

    #[test]
    fn parses_spark_envelope() {
        let quotes = parse_quote_response(
            br#"{"spark":{"result":[{"symbol":"aapl","response":[{"meta":{"regularMarketPrice":151.25}}]}]}}"#,
        );
        assert_eq!(quotes.get("AAPL"), Some(&151.25));
    }

    #[test]
    fn unknown_payload_yields_no_quotes() {
        assert!(parse_quote_response(br#"{"message":"nope"}"#).is_empty());
        assert!(parse_quote_response(b"not json").is_empty());
    }
}
