//! Shared offset/limit pagination for SeaORM `Select` queries.

use sea_orm::{ConnectionTrait, DbErr, EntityTrait, PaginatorTrait, QuerySelect, Select};

/// Runs `query` as one offset-paginated page and returns `(rows, total)`.
///
/// `total` counts every row the query matches: it is taken before
/// `offset`/`limit` are applied (SeaORM's `count` also drops `ORDER BY`), so
/// apply filters, joins and ordering to `query` before calling. Two queries are
/// issued, `COUNT(*)` then the page, matching echobackend's list endpoints.
pub async fn paginate<E, C>(
    db: &C,
    query: Select<E>,
    offset: u64,
    limit: u64,
) -> Result<(Vec<E::Model>, u64), DbErr>
where
    E: EntityTrait,
    E::Model: Sync,
    C: ConnectionTrait,
{
    let total = query.clone().count(db).await?;
    let rows = query.offset(offset).limit(limit).all(db).await?;
    Ok((rows, total))
}
