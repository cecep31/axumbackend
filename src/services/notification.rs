use crate::entities::notifications;
use crate::models::notification::{MarkAllReadResponse, NotificationResponse, UnreadCountResponse};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, PaginatorTrait,
    QueryFilter, QueryOrder, QuerySelect, Set,
};
use uuid::Uuid;

pub async fn get_notifications(
    db: &DatabaseConnection,
    user_id: Uuid,
    unread_only: bool,
    limit: i64,
    offset: i64,
) -> Result<(Vec<NotificationResponse>, i64), DbErr> {
    let mut query = notifications::Entity::find().filter(notifications::Column::UserId.eq(user_id));
    if unread_only {
        query = query.filter(notifications::Column::Read.eq(false));
    }

    let total = query.clone().count(db).await? as i64;
    let notifications = query
        .order_by_desc(notifications::Column::CreatedAt)
        .limit(limit.max(0) as u64)
        .offset(offset.max(0) as u64)
        .all(db)
        .await?
        .into_iter()
        .map(Into::into)
        .collect();

    Ok((notifications, total))
}

pub async fn get_unread_count(
    db: &DatabaseConnection,
    user_id: Uuid,
) -> Result<UnreadCountResponse, DbErr> {
    let count = notifications::Entity::find()
        .filter(notifications::Column::UserId.eq(user_id))
        .filter(notifications::Column::Read.eq(false))
        .count(db)
        .await? as i64;

    Ok(UnreadCountResponse {
        unread_count: count,
    })
}

pub async fn mark_as_read(
    db: &DatabaseConnection,
    id: Uuid,
    user_id: Uuid,
) -> Result<Option<NotificationResponse>, DbErr> {
    let Some(notification) = notifications::Entity::find_by_id(id)
        .filter(notifications::Column::UserId.eq(user_id))
        .one(db)
        .await?
    else {
        return Ok(None);
    };

    let mut active: notifications::ActiveModel = notification.into();
    active.read = Set(true);
    active.updated_at = Set(Utc::now().into());
    let updated = active.update(db).await?;

    Ok(Some(updated.into()))
}

pub async fn mark_all_as_read(
    db: &DatabaseConnection,
    user_id: Uuid,
) -> Result<MarkAllReadResponse, DbErr> {
    let result = notifications::Entity::update_many()
        .col_expr(notifications::Column::Read, true.into())
        .col_expr(notifications::Column::UpdatedAt, Utc::now().into())
        .filter(notifications::Column::UserId.eq(user_id))
        .filter(notifications::Column::Read.eq(false))
        .exec(db)
        .await?;

    Ok(MarkAllReadResponse {
        updated_count: result.rows_affected as i64,
    })
}
