use super::{HoldingError, parse_decimal, round2};
use crate::clients::market_quote;
use crate::config::MarketConfig;
use crate::entities::holdings;
use crate::models::holding::*;
use chrono::{Datelike, Utc};
use sea_orm::prelude::{Decimal, Expr};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use uuid::Uuid;

/// Refreshes `current_price` / `current_value` for the current month's
/// holdings that have a symbol and positive units, using live market quotes.
/// Mirrors echobackend's `holdingService.SyncPrices`.
pub async fn sync_prices(
    db: &DatabaseConnection,
    user_id: Uuid,
) -> Result<HoldingSyncResponse, HoldingError> {
    let now = Utc::now();
    let (month, year) = (now.month() as i32, now.year());

    let holdings = holdings::Entity::find()
        .filter(holdings::Column::UserId.eq(user_id))
        .filter(holdings::Column::Month.eq(month))
        .filter(holdings::Column::Year.eq(year))
        .filter(holdings::Column::Symbol.is_not_null())
        .filter(holdings::Column::Symbol.ne(""))
        .filter(holdings::Column::Units.gt(Decimal::ZERO))
        .all(db)
        .await?;

    if holdings.is_empty() {
        return Ok(HoldingSyncResponse {
            synced_count: 0,
            month,
            year,
        });
    }

    let symbols: Vec<&str> = holdings
        .iter()
        .filter_map(|holding| holding.symbol.as_deref())
        .collect();
    let quotes = market_quote::get_quotes(&MarketConfig::get().rapidapi_key, &symbols)
        .await
        .map_err(HoldingError::Quote)?;

    let mut synced_count = 0;
    for holding in holdings {
        let (Some(symbol), Some(units)) = (holding.symbol.as_deref(), holding.units) else {
            continue;
        };
        let Some(&price) = quotes.get(&symbol.trim().to_uppercase()) else {
            continue;
        };
        if price <= 0.0 {
            continue;
        }
        let units: f64 = match units.to_string().parse() {
            Ok(units) if units > 0.0 => units,
            _ => continue,
        };

        let current_value = round2(units * price);
        let result = holdings::Entity::update_many()
            .col_expr(
                holdings::Column::CurrentPrice,
                Expr::value(parse_decimal(&format!("{price:.8}"), "current_price")?),
            )
            .col_expr(
                holdings::Column::CurrentValue,
                Expr::value(parse_decimal(
                    &format!("{current_value:.2}"),
                    "current_value",
                )?),
            )
            .col_expr(holdings::Column::LastUpdated, Expr::value(now))
            .col_expr(holdings::Column::UpdatedAt, Expr::value(now))
            .filter(holdings::Column::Id.eq(holding.id))
            .filter(holdings::Column::UserId.eq(user_id))
            .exec(db)
            .await?;
        if result.rows_affected == 0 {
            return Err(HoldingError::NotFound);
        }
        synced_count += 1;
    }

    Ok(HoldingSyncResponse {
        synced_count,
        month,
        year,
    })
}
