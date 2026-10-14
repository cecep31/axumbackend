use super::*;

pub(super) fn activity_response(log: auth_activity_logs::Model) -> AuthActivityLogResponse {
    AuthActivityLogResponse {
        id: log.id,
        user_id: log.user_id,
        activity_type: log.activity_type,
        ip_address: log.ip_address,
        user_agent: log.user_agent,
        status: log.status,
        error_message: log.error_message,
        metadata: log.metadata,
        created_at: log.created_at.with_timezone(&Utc),
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn log_activity(
    db: &DatabaseConnection,
    user_id: Option<Uuid>,
    activity_type: &str,
    status: &str,
    ip_address: Option<String>,
    user_agent: Option<String>,
    error_message: Option<String>,
    metadata: Option<serde_json::Value>,
) {
    let log = auth_activity_logs::ActiveModel {
        id: Set(Uuid::now_v7()),
        user_id: Set(user_id),
        activity_type: Set(activity_type.to_string()),
        ip_address: Set(ip_address),
        user_agent: Set(user_agent),
        status: Set(status.to_string()),
        error_message: Set(error_message),
        metadata: Set(metadata),
        created_at: Set(Utc::now().into()),
    };

    if let Err(err) = log.insert(db).await {
        tracing::warn!(?err, activity_type, "failed to log auth activity");
    }
}

pub async fn get_activity_logs(
    db: &DatabaseConnection,
    user_id: Uuid,
    activity_type: Option<String>,
    limit: u64,
    offset: u64,
) -> Result<(Vec<AuthActivityLogResponse>, i64), AuthError> {
    let mut query =
        auth_activity_logs::Entity::find().filter(auth_activity_logs::Column::UserId.eq(user_id));

    if let Some(activity_type) = activity_type {
        query = query.filter(auth_activity_logs::Column::ActivityType.eq(activity_type));
    }

    let total = query.clone().count(db).await? as i64;
    let logs = query
        .order_by_desc(auth_activity_logs::Column::CreatedAt)
        .offset(offset)
        .limit(limit)
        .all(db)
        .await?
        .into_iter()
        .map(activity_response)
        .collect();

    Ok((logs, total))
}

pub async fn get_recent_activity(
    db: &DatabaseConnection,
    user_id: Uuid,
    limit: u64,
) -> Result<Vec<AuthActivityLogResponse>, AuthError> {
    let logs = auth_activity_logs::Entity::find()
        .filter(auth_activity_logs::Column::UserId.eq(user_id))
        .order_by_desc(auth_activity_logs::Column::CreatedAt)
        .limit(limit)
        .all(db)
        .await?
        .into_iter()
        .map(activity_response)
        .collect();

    Ok(logs)
}

pub async fn get_failed_logins(
    db: &DatabaseConnection,
    since_hours: i64,
    limit: u64,
    offset: u64,
) -> Result<(Vec<AuthActivityLogResponse>, i64), AuthError> {
    let since = Utc::now() - Duration::hours(since_hours);
    let query = auth_activity_logs::Entity::find()
        .filter(auth_activity_logs::Column::ActivityType.eq("login_failed"))
        .filter(auth_activity_logs::Column::CreatedAt.gte(since));

    let total = query.clone().count(db).await? as i64;
    let logs = query
        .order_by_desc(auth_activity_logs::Column::CreatedAt)
        .offset(offset)
        .limit(limit)
        .all(db)
        .await?
        .into_iter()
        .map(activity_response)
        .collect();

    Ok((logs, total))
}
