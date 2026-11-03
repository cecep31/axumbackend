//! IDX corporate-actions client (Indonesia Stock Exchange via RapidAPI).
//!
//! Ports the HTTP half of echobackend's `corporateActionService`: the dividend
//! and RUPS calendar endpoints and their response envelopes. Mapping the items
//! into corporate actions and persisting them stays in
//! `services::corporate_action`.

use chrono::NaiveDate;
use reqwest::Client;
use serde::Deserialize;
use std::sync::LazyLock;
use std::time::Duration;

const RAPIDAPI_IDX_HOST: &str = "indonesia-stock-exchange-idx.p.rapidapi.com";
const RAPIDAPI_IDX_BASE_URL: &str = "https://indonesia-stock-exchange-idx.p.rapidapi.com";
const DATE_FORMAT: &str = "%Y-%m-%d";

fn shared_client() -> &'static Client {
    static CLIENT: LazyLock<Client> = LazyLock::new(|| {
        Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .unwrap_or_else(|_| Client::new())
    });
    &CLIENT
}

// --- RapidAPI IDX response envelopes ----------------------------------------

#[derive(Debug, Deserialize)]
struct DividendEnvelope {
    data: DividendEnvelopeData,
}
#[derive(Debug, Deserialize)]
struct DividendEnvelopeData {
    data: DividendEnvelopeInner,
}
#[derive(Debug, Deserialize)]
struct DividendEnvelopeInner {
    dividend: Vec<RapidApiDividendItem>,
}

#[derive(Debug, Deserialize)]
struct RupsEnvelope {
    data: RupsEnvelopeData,
}
#[derive(Debug, Deserialize)]
struct RupsEnvelopeData {
    data: RupsEnvelopeInner,
}
#[derive(Debug, Deserialize)]
struct RupsEnvelopeInner {
    rups: Vec<RapidApiRupsItem>,
}

/// Go `encoding/json` into a `string`: a missing key or `null` leaves "".
fn go_string<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_default())
}

/// One row of `/api/calendar/dividend`; every field is a raw string ("" when
/// missing or `null`).
#[derive(Debug, Deserialize)]
pub struct RapidApiDividendItem {
    #[serde(default, deserialize_with = "go_string")]
    pub company_symbol: String,
    #[serde(default, deserialize_with = "go_string")]
    pub dividend_exdate: String,
    #[serde(default, deserialize_with = "go_string")]
    pub dividend_cumdate: String,
    #[serde(default, deserialize_with = "go_string")]
    pub dividend_recdate: String,
    #[serde(default, deserialize_with = "go_string")]
    pub dividend_paydate: String,
    #[serde(default, deserialize_with = "go_string")]
    pub dividend_value: String,
    #[serde(default, deserialize_with = "go_string")]
    pub dividend_currency: String,
}

/// One row of `/api/calendar/rups` (shareholder meetings); every field is a
/// raw string ("" when missing or `null`).
#[derive(Debug, Deserialize)]
pub struct RapidApiRupsItem {
    #[serde(default, deserialize_with = "go_string")]
    pub company_symbol: String,
    #[serde(default, deserialize_with = "go_string")]
    pub company_name: String,
    #[serde(default, deserialize_with = "go_string")]
    pub rups_date: String,
    #[serde(default, deserialize_with = "go_string")]
    pub rups_time: String,
    #[serde(default, deserialize_with = "go_string")]
    pub rups_venue: String,
}

/// Fetches the dividend calendar for `[from, to]`.
pub async fn fetch_dividends(
    api_key: &str,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<Vec<RapidApiDividendItem>, String> {
    let body = do_request(api_key, "/api/calendar/dividend", from, to).await?;
    let envelope: DividendEnvelope = serde_json::from_slice(body.as_slice())
        .map_err(|err| format!("decode dividend response: {err}"))?;
    Ok(envelope.data.data.dividend)
}

/// Fetches the RUPS (shareholder meeting) calendar for `[from, to]`.
pub async fn fetch_rups(
    api_key: &str,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<Vec<RapidApiRupsItem>, String> {
    let body = do_request(api_key, "/api/calendar/rups", from, to).await?;
    let envelope: RupsEnvelope = serde_json::from_slice(body.as_slice())
        .map_err(|err| format!("decode rups response: {err}"))?;
    Ok(envelope.data.data.rups)
}

async fn do_request(
    api_key: &str,
    endpoint: &str,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<Vec<u8>, String> {
    let from = from.format(DATE_FORMAT);
    let to = to.format(DATE_FORMAT);
    let url = format!("{RAPIDAPI_IDX_BASE_URL}{endpoint}?from={from}&to={to}");
    let response = shared_client()
        .get(&url)
        .header("X-RapidAPI-Key", api_key)
        .header("X-RapidAPI-Host", RAPIDAPI_IDX_HOST)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|err| format!("request {endpoint}: {err}"))?;

    let status = response.status();
    let body = response
        .bytes()
        .await
        .map_err(|err| format!("read {endpoint} response: {err}"))?;
    if !status.is_success() {
        let lossy = String::from_utf8_lossy(&body);
        return Err(format!(
            "rapidapi idx {endpoint} returned status {status}: {}",
            truncate(&lossy, 200)
        ));
    }
    Ok(body.to_vec())
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
    fn rapidapi_items_tolerate_null_and_missing_fields() {
        let item: RapidApiDividendItem = serde_json::from_str(
            r#"{"company_symbol":"BBCA","dividend_exdate":"2025-07-15","dividend_cumdate":"2025-07-14","dividend_recdate":null,"dividend_value":null}"#,
        )
        .unwrap();
        assert_eq!(item.dividend_cumdate, "2025-07-14");
        assert!(item.dividend_recdate.is_empty());
        assert!(item.dividend_value.is_empty());
        assert!(item.dividend_paydate.is_empty());

        let rups: RapidApiRupsItem = serde_json::from_str(r#"{"company_symbol":null}"#).unwrap();
        assert!(rups.company_symbol.is_empty());
    }
}
