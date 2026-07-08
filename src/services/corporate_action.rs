//! Corporate-actions calendar service.
//!
//! Ports echobackend's `corporateActionService` + `pkg/market.RapidAPIIDXClient`:
//! fetches IDX dividend and RUPS calendars from RapidAPI, merges and sorts them,
//! and caches the result in memory for 6 hours. External API errors are
//! swallowed (fail-open) so a partial result is always returned.

use crate::config::MarketConfig;
use crate::models::corporate_action::{CorporateActionCalendarResponse, CorporateActionResponse};
use chrono::{Datelike, NaiveDate};
use once_cell::sync::Lazy;
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const RAPIDAPI_IDX_HOST: &str = "indonesia-stock-exchange-idx.p.rapidapi.com";
const RAPIDAPI_IDX_BASE_URL: &str = "https://indonesia-stock-exchange-idx.p.rapidapi.com";
const DATE_FORMAT: &str = "%Y-%m-%d";

/// How long the calendar result is cached in memory.
/// Matches echobackend's 6-hour Redis TTL (`corporateActionCacheTTL`).
const CACHE_TTL: Duration = Duration::from_secs(6 * 60 * 60);
/// Caps the accepted from-to range. Matches echobackend's `maxCalendarRangeMonths`.
const MAX_CALENDAR_RANGE_MONTHS: i64 = 6;

#[derive(Debug, Clone)]
struct CorporateAction {
    symbol: String,
    name: String,
    action_type: String,
    date: NaiveDate,
    pay_date: Option<NaiveDate>,
    amount: Option<f64>,
    currency: Option<String>,
    note: Option<String>,
    market: String,
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

#[derive(Debug, Deserialize)]
struct RapidApiDividendItem {
    company_symbol: String,
    dividend_exdate: String,
    dividend_paydate: String,
    dividend_value: String,
    dividend_currency: String,
}

#[derive(Debug, Deserialize)]
struct RapidApiRupsItem {
    company_symbol: String,
    company_name: String,
    rups_date: String,
    rups_time: String,
    rups_venue: String,
}

/// In-memory cache: key `"from|to"` -> `(stored_at, response)`.
static CACHE: Lazy<Mutex<HashMap<String, (Instant, CorporateActionCalendarResponse)>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Add `months` calendar months to `date`, clamping the day to the last valid
/// day of the target month (e.g. Jan 31 + 1 month -> Feb 28/29).
pub fn add_months(date: NaiveDate, months: i64) -> NaiveDate {
    let total = date.year() as i64 * 12 + (date.month() as i64 - 1) + months;
    let new_year = total.div_euclid(12) as i32;
    let new_month = total.rem_euclid(12) as u32 + 1;
    (1..=date.day())
        .rev()
        .find_map(|d| NaiveDate::from_ymd_opt(new_year, new_month, d))
        .expect("a valid day always exists within any month")
}

/// Returns corporate actions (dividend + RUPS) within the `[from, to]` range.
///
/// Results are cached in memory for [`CACHE_TTL`]. External API errors are
/// swallowed (fail-open) so a partial result is always returned - this mirrors
/// echobackend's `corporateActionService.GetCalendar` behaviour.
pub async fn get_calendar(from: NaiveDate, to: NaiveDate) -> CorporateActionCalendarResponse {
    // Cap range to MAX_CALENDAR_RANGE_MONTHS.
    let max_to = add_months(from, MAX_CALENDAR_RANGE_MONTHS);
    let to = if to > max_to { max_to } else { to };

    let from_str = from.format(DATE_FORMAT).to_string();
    let to_str = to.format(DATE_FORMAT).to_string();
    let cache_key = format!("{from_str}|{to_str}");

    // Try cache.
    if let Ok(cache) = CACHE.lock()
        && let Some((stored_at, cached)) = cache.get(&cache_key)
        && stored_at.elapsed() < CACHE_TTL
    {
        let mut result = cached.clone();
        result.cached = true;
        return result;
    }

    let api_key = MarketConfig::get().rapidapi_idx_key.clone();
    let client = shared_client();
    let mut actions = fetch_corporate_actions(&client, &api_key, from, to).await;

    // Sort by date ascending.
    actions.sort_by_key(|a| a.date);

    let responses: Vec<CorporateActionResponse> = actions
        .into_iter()
        .map(|a| CorporateActionResponse {
            symbol: a.symbol,
            name: a.name,
            action_type: a.action_type,
            date: a.date.format(DATE_FORMAT).to_string(),
            pay_date: a.pay_date.map(|d| d.format(DATE_FORMAT).to_string()),
            amount: a.amount,
            currency: a.currency,
            note: a.note,
            market: a.market,
        })
        .collect();

    let result = CorporateActionCalendarResponse {
        from: from_str,
        to: to_str,
        total: responses.len(),
        cached: false,
        actions: responses,
    };

    // Persist to cache (best-effort).
    if let Ok(mut cache) = CACHE.lock() {
        cache.insert(cache_key, (Instant::now(), result.clone()));
    }

    result
}

fn shared_client() -> Client {
    static CLIENT: Lazy<Option<Client>> = Lazy::new(|| {
        Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .ok()
    });
    CLIENT.as_ref().cloned().unwrap_or_else(Client::new)
}

async fn fetch_corporate_actions(
    client: &Client,
    api_key: &str,
    from: NaiveDate,
    to: NaiveDate,
) -> Vec<CorporateAction> {
    if api_key.is_empty() {
        return Vec::new();
    }
    let from_str = from.format(DATE_FORMAT).to_string();
    let to_str = to.format(DATE_FORMAT).to_string();
    let mut actions = Vec::new();

    // Dividends
    match fetch_dividends(client, api_key, &from_str, &to_str).await {
        Ok(dividends) => {
            for d in dividends {
                if d.company_symbol.is_empty() {
                    continue;
                }
                let sym = normalize_idx_symbol(&d.company_symbol);
                let Some(ex_date) = parse_date(&d.dividend_exdate) else {
                    continue;
                };
                actions.push(CorporateAction {
                    symbol: sym.clone(),
                    name: sym,
                    action_type: "dividend".to_string(),
                    date: ex_date,
                    pay_date: parse_date(&d.dividend_paydate),
                    amount: d
                        .dividend_value
                        .parse::<f64>()
                        .ok()
                        .filter(|value| *value > 0.0),
                    currency: Some(normalize_currency(&d.dividend_currency)),
                    note: None,
                    market: "IDX".to_string(),
                });
            }
        }
        Err(err) => tracing::warn!(?err, "rapidapi idx dividend fetch failed; skipping"),
    }

    // RUPS
    match fetch_rups(client, api_key, &from_str, &to_str).await {
        Ok(rups_list) => {
            for r in rups_list {
                if r.company_symbol.is_empty() {
                    continue;
                }
                let sym = normalize_idx_symbol(&r.company_symbol);
                let Some(meeting_date) = parse_date(&r.rups_date) else {
                    continue;
                };
                let name = if r.company_name.is_empty() {
                    sym.clone()
                } else {
                    r.company_name
                };
                let note = if !r.rups_venue.is_empty() {
                    if !r.rups_time.is_empty() {
                        Some(format!("Waktu: {}, Tempat: {}", r.rups_time, r.rups_venue))
                    } else {
                        Some(format!("Tempat: {}", r.rups_venue))
                    }
                } else {
                    None
                };
                actions.push(CorporateAction {
                    symbol: sym,
                    name,
                    action_type: "rups".to_string(),
                    date: meeting_date,
                    pay_date: None,
                    amount: None,
                    currency: None,
                    note,
                    market: "IDX".to_string(),
                });
            }
        }
        Err(err) => tracing::warn!(?err, "rapidapi idx rups fetch failed; skipping"),
    }

    actions
}

async fn fetch_dividends(
    client: &Client,
    api_key: &str,
    from: &str,
    to: &str,
) -> Result<Vec<RapidApiDividendItem>, String> {
    let body = do_request(client, api_key, "/api/calendar/dividend", from, to).await?;
    let envelope: DividendEnvelope = serde_json::from_slice(body.as_slice())
        .map_err(|err| format!("decode dividend response: {err}"))?;
    Ok(envelope.data.data.dividend)
}

async fn fetch_rups(
    client: &Client,
    api_key: &str,
    from: &str,
    to: &str,
) -> Result<Vec<RapidApiRupsItem>, String> {
    let body = do_request(client, api_key, "/api/calendar/rups", from, to).await?;
    let envelope: RupsEnvelope = serde_json::from_slice(body.as_slice())
        .map_err(|err| format!("decode rups response: {err}"))?;
    Ok(envelope.data.data.rups)
}

async fn do_request(
    client: &Client,
    api_key: &str,
    endpoint: &str,
    from: &str,
    to: &str,
) -> Result<Vec<u8>, String> {
    let url = format!("{RAPIDAPI_IDX_BASE_URL}{endpoint}?from={from}&to={to}");
    let response = client
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

fn parse_date(s: &str) -> Option<NaiveDate> {
    if s.is_empty() {
        return None;
    }
    NaiveDate::parse_from_str(s, DATE_FORMAT).ok()
}

/// Uppercase, trim, and strip the Yahoo Finance `.JK` suffix from an IDX ticker.
fn normalize_idx_symbol(s: &str) -> String {
    let s = s.trim().to_uppercase();
    s.strip_suffix(".JK").unwrap_or(&s).to_string()
}

/// Normalise a currency code: uppercase, strip the `CURRENCY_` prefix, default
/// to `IDR` when empty.
fn normalize_currency(c: &str) -> String {
    let c = c.trim().to_uppercase();
    let c = c.strip_prefix("CURRENCY_").unwrap_or(&c);
    if c.is_empty() {
        "IDR".to_string()
    } else {
        c.to_string()
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
