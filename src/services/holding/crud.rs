use super::{
    CreateHoldingInput, HoldingError, UpdateHoldingInput, parse_decimal, parse_optional_decimal,
    parse_optional_time,
};
use crate::entities::{holding_types, holdings};
use crate::models::holding::*;
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, IntoActiveModel,
    QueryFilter, QueryOrder, Set, TransactionTrait,
};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

/// Builds the responses for `holdings` (in their order) with one
/// `holding_types` lookup for the whole list.
async fn hydrate_many(
    db: &DatabaseConnection,
    holdings: Vec<holdings::Model>,
) -> Result<Vec<HoldingResponse>, DbErr> {
    let type_ids: HashSet<i16> = holdings.iter().map(|h| h.holding_type_id).collect();
    let types_by_id: HashMap<i16, holding_types::Model> = if type_ids.is_empty() {
        HashMap::new()
    } else {
        holding_types::Entity::find()
            .filter(holding_types::Column::Id.is_in(type_ids))
            .all(db)
            .await?
            .into_iter()
            .map(|t| (t.id, t))
            .collect()
    };
    Ok(holdings
        .into_iter()
        .map(|holding| {
            let holding_type = types_by_id.get(&holding.holding_type_id).cloned();
            HoldingResponse::from_entity(holding, holding_type)
        })
        .collect())
}

async fn hydrate(
    db: &DatabaseConnection,
    holding: holdings::Model,
) -> Result<HoldingResponse, DbErr> {
    hydrate_many(db, vec![holding])
        .await?
        .pop()
        .ok_or_else(|| DbErr::Custom("holding hydration returned no row".to_string()))
}

async fn holding_type_exists(db: &DatabaseConnection, id: i16) -> Result<bool, DbErr> {
    Ok(holding_types::Entity::find_by_id(id)
        .one(db)
        .await?
        .is_some())
}

/// Redis key/TTL shared with echobackend's `GetHoldingTypes`
/// (`<prefix>:holding-types`, a JSON array of `{id, code, name, notes}`).
const HOLDING_TYPES_CACHE_KEY: [&str; 1] = ["holding-types"];
const HOLDING_TYPES_TTL: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

pub async fn get_holding_types(
    db: &DatabaseConnection,
) -> Result<Vec<HoldingTypeResponse>, HoldingError> {
    if let Some(types) =
        crate::infra::cache::read_json::<Vec<HoldingTypeResponse>>(&HOLDING_TYPES_CACHE_KEY).await
    {
        return Ok(types);
    }
    let types: Vec<HoldingTypeResponse> = holding_types::Entity::find()
        .order_by_asc(holding_types::Column::Id)
        .all(db)
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    crate::infra::cache::write_json(&HOLDING_TYPES_CACHE_KEY, &types, HOLDING_TYPES_TTL).await;
    Ok(types)
}

pub async fn get_holdings(
    db: &DatabaseConnection,
    user_id: Uuid,
    month: Option<i32>,
    year: Option<i32>,
    sort_by: Option<&str>,
    order: Option<&str>,
) -> Result<Vec<HoldingResponse>, HoldingError> {
    let mut query = holdings::Entity::find().filter(holdings::Column::UserId.eq(user_id));
    if let Some(month) = month {
        query = query.filter(holdings::Column::Month.eq(month));
    }
    if let Some(year) = year {
        query = query.filter(holdings::Column::Year.eq(year));
    }

    let desc = !matches!(order, Some("asc"));
    let sort_field = sort_by.unwrap_or("created_at");
    query = match sort_field {
        "updated_at" => {
            if desc {
                query.order_by_desc(holdings::Column::UpdatedAt)
            } else {
                query.order_by_asc(holdings::Column::UpdatedAt)
            }
        }
        "name" => {
            if desc {
                query.order_by_desc(holdings::Column::Name)
            } else {
                query.order_by_asc(holdings::Column::Name)
            }
        }
        "platform" => {
            if desc {
                query.order_by_desc(holdings::Column::Platform)
            } else {
                query.order_by_asc(holdings::Column::Platform)
            }
        }
        "invested_amount" => {
            if desc {
                query.order_by_desc(holdings::Column::InvestedAmount)
            } else {
                query.order_by_asc(holdings::Column::InvestedAmount)
            }
        }
        "current_value" => {
            if desc {
                query.order_by_desc(holdings::Column::CurrentValue)
            } else {
                query.order_by_asc(holdings::Column::CurrentValue)
            }
        }
        "holding_type" => {
            if desc {
                query.order_by_desc(holdings::Column::HoldingTypeId)
            } else {
                query.order_by_asc(holdings::Column::HoldingTypeId)
            }
        }
        _ => {
            if desc {
                query.order_by_desc(holdings::Column::CreatedAt)
            } else {
                query.order_by_asc(holdings::Column::CreatedAt)
            }
        }
    };
    // echobackend tiebreaks the `holding_type` sort on `created_at`; every
    // other field tiebreaks on `id`.
    query = if sort_field == "holding_type" {
        if desc {
            query.order_by_desc(holdings::Column::CreatedAt)
        } else {
            query.order_by_asc(holdings::Column::CreatedAt)
        }
    } else if desc {
        query.order_by_desc(holdings::Column::Id)
    } else {
        query.order_by_asc(holdings::Column::Id)
    };

    let models = query.all(db).await?;
    Ok(hydrate_many(db, models).await?)
}

pub async fn get_holding_by_id(
    db: &DatabaseConnection,
    id: i64,
    user_id: Uuid,
) -> Result<HoldingResponse, HoldingError> {
    let Some(model) = holdings::Entity::find_by_id(id)
        .filter(holdings::Column::UserId.eq(user_id))
        .one(db)
        .await?
    else {
        return Err(HoldingError::NotFound);
    };
    Ok(hydrate(db, model).await?)
}

pub async fn create_holding(
    db: &DatabaseConnection,
    user_id: Uuid,
    input: CreateHoldingInput,
) -> Result<HoldingResponse, HoldingError> {
    if !holding_type_exists(db, input.holding_type_id).await? {
        return Err(HoldingError::HoldingTypeNotFound);
    }

    let now = Utc::now().into();
    let model = holdings::ActiveModel {
        user_id: Set(user_id),
        name: Set(input.name),
        symbol: Set(input.symbol),
        platform: Set(input.platform),
        holding_type_id: Set(input.holding_type_id),
        currency: Set(input.currency),
        invested_amount: Set(parse_decimal(&input.invested_amount, "invested_amount")?),
        current_value: Set(parse_decimal(&input.current_value, "current_value")?),
        units: Set(parse_optional_decimal(input.units, "units")?),
        avg_buy_price: Set(parse_optional_decimal(
            input.avg_buy_price,
            "avg_buy_price",
        )?),
        current_price: Set(parse_optional_decimal(
            input.current_price,
            "current_price",
        )?),
        last_updated: Set(parse_optional_time(input.last_updated)),
        notes: Set(input.notes),
        created_at: Set(now),
        updated_at: Set(now),
        month: Set(input.month),
        year: Set(input.year),
        ..Default::default()
    }
    .insert(db)
    .await?;

    Ok(hydrate(db, model).await?)
}

pub async fn update_holding(
    db: &DatabaseConnection,
    id: i64,
    user_id: Uuid,
    input: UpdateHoldingInput,
) -> Result<HoldingResponse, HoldingError> {
    let Some(existing) = holdings::Entity::find_by_id(id)
        .filter(holdings::Column::UserId.eq(user_id))
        .one(db)
        .await?
    else {
        return Err(HoldingError::NotFound);
    };

    if let Some(holding_type_id) = input.holding_type_id
        && !holding_type_exists(db, holding_type_id).await?
    {
        return Err(HoldingError::HoldingTypeNotFound);
    }

    let mut active = existing.into_active_model();
    if let Some(value) = input.name {
        active.name = Set(value);
    }
    if input.symbol.is_some() {
        active.symbol = Set(input.symbol);
    }
    if let Some(value) = input.platform {
        active.platform = Set(value);
    }
    if let Some(value) = input.holding_type_id {
        active.holding_type_id = Set(value);
    }
    if let Some(value) = input.currency {
        active.currency = Set(value);
    }
    if let Some(value) = input.invested_amount {
        active.invested_amount = Set(parse_decimal(&value, "invested_amount")?);
    }
    if let Some(value) = input.current_value {
        active.current_value = Set(parse_decimal(&value, "current_value")?);
    }
    if input.units.is_some() {
        active.units = Set(parse_optional_decimal(input.units, "units")?);
    }
    if input.avg_buy_price.is_some() {
        active.avg_buy_price = Set(parse_optional_decimal(
            input.avg_buy_price,
            "avg_buy_price",
        )?);
    }
    if input.current_price.is_some() {
        active.current_price = Set(parse_optional_decimal(
            input.current_price,
            "current_price",
        )?);
    }
    if input.last_updated.is_some() {
        active.last_updated = Set(parse_optional_time(input.last_updated));
    }
    if input.notes.is_some() {
        active.notes = Set(input.notes);
    }
    if let Some(value) = input.month {
        active.month = Set(value);
    }
    if let Some(value) = input.year {
        active.year = Set(value);
    }
    active.updated_at = Set(Utc::now().into());

    let updated = active.update(db).await?;
    Ok(hydrate(db, updated).await?)
}

pub async fn delete_holding(
    db: &DatabaseConnection,
    id: i64,
    user_id: Uuid,
) -> Result<(), HoldingError> {
    let result = holdings::Entity::delete_many()
        .filter(holdings::Column::Id.eq(id))
        .filter(holdings::Column::UserId.eq(user_id))
        .exec(db)
        .await?;
    if result.rows_affected == 0 {
        return Err(HoldingError::NotFound);
    }
    Ok(())
}

pub async fn duplicate_holdings(
    db: &DatabaseConnection,
    user_id: Uuid,
    from_month: i32,
    from_year: i32,
    to_month: i32,
    to_year: i32,
    overwrite: bool,
) -> Result<Vec<DuplicateResultItem>, HoldingError> {
    if from_month == to_month && from_year == to_year {
        return Err(HoldingError::DuplicateSameMonth);
    }
    let source = holdings::Entity::find()
        .filter(holdings::Column::UserId.eq(user_id))
        .filter(holdings::Column::Month.eq(from_month))
        .filter(holdings::Column::Year.eq(from_year))
        .all(db)
        .await?;
    if source.is_empty() {
        return Err(HoldingError::NotFound);
    }
    // Delete and re-insert atomically: a failed insert must not leave the
    // target month empty or half-copied.
    let txn = db.begin().await?;
    if overwrite {
        holdings::Entity::delete_many()
            .filter(holdings::Column::UserId.eq(user_id))
            .filter(holdings::Column::Month.eq(to_month))
            .filter(holdings::Column::Year.eq(to_year))
            .exec(&txn)
            .await?;
    }
    let now: chrono::DateTime<chrono::FixedOffset> = Utc::now().into();
    let rows = source.into_iter().map(|item| holdings::ActiveModel {
        user_id: Set(user_id),
        name: Set(item.name),
        symbol: Set(item.symbol),
        platform: Set(item.platform),
        holding_type_id: Set(item.holding_type_id),
        currency: Set(item.currency),
        invested_amount: Set(item.invested_amount),
        current_value: Set(item.current_value),
        units: Set(item.units),
        avg_buy_price: Set(item.avg_buy_price),
        current_price: Set(item.current_price),
        notes: Set(item.notes),
        month: Set(to_month),
        year: Set(to_year),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    });
    let created = holdings::Entity::insert_many(rows)
        .exec_with_returning(&txn)
        .await?;
    txn.commit().await?;

    Ok(created
        .into_iter()
        .map(|created| DuplicateResultItem {
            id: created.id.to_string(),
            name: created.name,
            month: created.month,
            year: created.year,
        })
        .collect())
}
