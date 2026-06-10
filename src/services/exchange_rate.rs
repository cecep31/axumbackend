use crate::dto::exchange_rate::ExchangeRateResponse;
use chrono::Utc;
use serde::Deserialize;
use std::collections::HashMap;

const YAHOO_SPARK_URL: &str = "https://query1.finance.yahoo.com/v7/finance/spark";

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

pub async fn get_rate(from: String, to: String) -> Result<ExchangeRateResponse, ExchangeRateError> {
    let from = normalize_currency_code(&from);
    let to = normalize_currency_code(&to);
    if !valid_currency_code(&from) || !valid_currency_code(&to) {
        return Err(ExchangeRateError::InvalidCurrencyPair);
    }

    if from == to {
        return Ok(response(&from, &to, &format!("{from}{to}=X"), 1.0));
    }

    let direct_symbol = yahoo_currency_symbol(&from, &to);
    let inverse_symbol = yahoo_currency_symbol(&to, &from);
    let quotes = fetch_quotes(&[direct_symbol.clone(), inverse_symbol.clone()]).await?;

    if let Some(rate) = quotes
        .get(&direct_symbol)
        .copied()
        .filter(|rate| *rate > 0.0)
    {
        return Ok(response(&from, &to, &direct_symbol, rate));
    }

    if let Some(inverse_rate) = quotes
        .get(&inverse_symbol)
        .copied()
        .filter(|rate| *rate > 0.0)
    {
        let rate = ((1.0 / inverse_rate) * 100_000_000.0).round() / 100_000_000.0;
        return Ok(response(&from, &to, &inverse_symbol, rate));
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
