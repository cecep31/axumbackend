//! Corporate-actions calendar service (dividend & RUPS).
//!
//! Ports echobackend's `corporateActionService`: results are persisted in
//! Postgres (`corporate_actions` table). If the requested month already has
//! stored rows they are served directly (`cached: true`); otherwise the IDX
//! API (RapidAPI) is queried, the results are upserted, and the stored rows
//! are returned. External API errors are swallowed (fail-open) so a partial
//! result is always returned.

use crate::config::MarketConfig;
use crate::entities::corporate_actions;
use crate::models::corporate_action::{CorporateActionCalendarResponse, CorporateActionResponse};
use chrono::{Datelike, NaiveDate, Utc};
use once_cell::sync::Lazy;
use reqwest::Client;
use sea_orm::prelude::Decimal;
use sea_orm::sea_query::OnConflict;
use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, Set,
};
use serde::Deserialize;
use std::collections::HashMap;
use std::time::Duration;

const RAPIDAPI_IDX_HOST: &str = "indonesia-stock-exchange-idx.p.rapidapi.com";
const RAPIDAPI_IDX_BASE_URL: &str = "https://indonesia-stock-exchange-idx.p.rapidapi.com";
const DATE_FORMAT: &str = "%Y-%m-%d";

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

/// Add `months` calendar months to `date`, clamping the day to the last valid
/// day of the target month (e.g. Jan 31 + 1 month -> Feb 28/29).
fn add_months(date: NaiveDate, months: i64) -> NaiveDate {
    let total = date.year() as i64 * 12 + (date.month() as i64 - 1) + months;
    let new_year = total.div_euclid(12) as i32;
    let new_month = total.rem_euclid(12) as u32 + 1;
    (1..=date.day())
        .rev()
        .find_map(|d| NaiveDate::from_ymd_opt(new_year, new_month, d))
        .expect("a valid day always exists within any month")
}

/// Returns the corporate-actions calendar for one month, mirroring
/// echobackend's `CorporateActionService.GetCalendar`.
pub async fn get_calendar(
    db: &DatabaseConnection,
    year: i32,
    month: i32,
) -> CorporateActionCalendarResponse {
    // echobackend: an invalid month falls back to the current one.
    let month = if (1..=12).contains(&month) {
        month as u32
    } else {
        Utc::now().month()
    };
    let from = NaiveDate::from_ymd_opt(year, month, 1).unwrap_or_else(|| {
        let now = Utc::now().date_naive();
        NaiveDate::from_ymd_opt(now.year(), now.month(), 1).expect("first day of month is valid")
    });
    let to = add_months(from, 1) - chrono::Duration::days(1);

    // Rows already stored for this month are served straight from Postgres
    // (`cached: true`), without calling IDX again.
    let cached = corporate_actions::Entity::find()
        .filter(corporate_actions::Column::EventDate.between(from, to))
        .count(db)
        .await
        .map(|count| count > 0)
        .unwrap_or(false);

    if !cached && !MarketConfig::get().rapidapi_idx_key.is_empty() {
        let client = shared_client();
        let actions =
            fetch_corporate_actions(client, &MarketConfig::get().rapidapi_idx_key, from, to).await;
        let models = dedupe_actions(actions)
            .into_iter()
            .map(|action| corporate_actions::ActiveModel {
                symbol: Set(action.symbol),
                name: Set(if action.name.is_empty() {
                    None
                } else {
                    Some(action.name)
                }),
                r#type: Set(action.action_type),
                event_date: Set(action.date),
                pay_date: Set(action.pay_date),
                amount: Set(action.amount.and_then(Decimal::from_f64_retain)),
                currency: Set(action.currency.filter(|value| !value.is_empty())),
                note: Set(action.note.filter(|value| !value.is_empty())),
                market: Set(action.market),
                ..Default::default()
            })
            .collect::<Vec<_>>();
        if !models.is_empty() {
            let insert_result = corporate_actions::Entity::insert_many(models)
                .on_conflict(
                    OnConflict::columns([
                        corporate_actions::Column::Symbol,
                        corporate_actions::Column::Type,
                        corporate_actions::Column::EventDate,
                    ])
                    .update_columns([
                        corporate_actions::Column::Name,
                        corporate_actions::Column::PayDate,
                        corporate_actions::Column::Amount,
                        corporate_actions::Column::Currency,
                        corporate_actions::Column::Note,
                        corporate_actions::Column::Market,
                        corporate_actions::Column::UpdatedAt,
                    ])
                    .to_owned(),
                )
                .exec(db)
                .await;
            if let Err(err) = insert_result {
                tracing::warn!(?err, "failed to upsert corporate actions");
            }
        }
    }

    let stored = corporate_actions::Entity::find()
        .filter(corporate_actions::Column::EventDate.between(from, to))
        .order_by_asc(corporate_actions::Column::EventDate)
        .all(db)
        .await
        .unwrap_or_default();

    let actions: Vec<CorporateActionResponse> = stored.into_iter().map(to_response).collect();
    CorporateActionCalendarResponse {
        from: from.format(DATE_FORMAT).to_string(),
        to: to.format(DATE_FORMAT).to_string(),
        total: actions.len(),
        cached,
        actions,
    }
}

fn to_response(row: corporate_actions::Model) -> CorporateActionResponse {
    CorporateActionResponse {
        symbol: row.symbol,
        name: row.name.filter(|value| !value.is_empty()),
        action_type: row.r#type,
        date: row.event_date.format(DATE_FORMAT).to_string(),
        pay_date: row
            .pay_date
            .map(|date| date.format(DATE_FORMAT).to_string()),
        amount: row
            .amount
            .and_then(|value| value.to_string().parse::<f64>().ok()),
        currency: row.currency.filter(|value| !value.is_empty()),
        note: row.note.filter(|value| !value.is_empty()),
        market: row.market,
    }
}

/// Collapses rows sharing (symbol, type, event_date) — the unique constraint
/// target — into one, mirroring echobackend's `dedupeCorporateActions`:
/// distinct notes are concatenated, and missing amount/pay_date are filled.
fn dedupe_actions(actions: Vec<CorporateAction>) -> Vec<CorporateAction> {
    let mut order: Vec<(String, String, NaiveDate)> = Vec::new();
    let mut merged: HashMap<(String, String, NaiveDate), CorporateAction> = HashMap::new();

    for mut action in actions {
        let key = (
            action.symbol.clone(),
            action.action_type.clone(),
            action.date,
        );
        let Some(existing) = merged.get_mut(&key) else {
            order.push(key.clone());
            merged.insert(key, action);
            continue;
        };
        let existing_note = existing.note.take().unwrap_or_default();
        let action_note = action.note.take().unwrap_or_default();
        if existing_note != action_note
            && !action_note.is_empty()
            && !existing_note.contains(&action_note)
        {
            existing.note = Some(if existing_note.is_empty() {
                action_note
            } else {
                format!("{existing_note}; {action_note}")
            });
        } else {
            existing.note = if existing_note.is_empty() {
                None
            } else {
                Some(existing_note)
            };
        }
        if existing.amount.is_none() {
            existing.amount = action.amount;
        }
        if existing.pay_date.is_none() {
            existing.pay_date = action.pay_date;
        }
    }

    order
        .into_iter()
        .filter_map(|key| merged.remove(&key))
        .collect()
}

fn shared_client() -> &'static Client {
    static CLIENT: Lazy<Client> = Lazy::new(|| {
        Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .unwrap_or_else(|_| Client::new())
    });
    &CLIENT
}

/// Fetches dividend + RUPS calendars from the IDX API. Individual API errors
/// are swallowed (fail-open) so a partial result is always returned.
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
