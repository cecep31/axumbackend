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
        metadata: log.metadata.as_ref().map(pg_jsonb_text),
        created_at: log.created_at.with_timezone(&Utc),
    }
}

/// Renders a JSON value the way PostgreSQL prints a `jsonb` column as text,
/// which is what echobackend returns for `metadata` (it scans the column into
/// a `*string`): `", "` / `": "` separators and object keys in jsonb storage
/// order (shorter keys first, then bytewise).
pub(super) fn pg_jsonb_text(value: &serde_json::Value) -> String {
    let mut out = String::new();
    write_pg_jsonb(value, &mut out);
    out
}

fn write_pg_jsonb(value: &serde_json::Value, out: &mut String) {
    use serde_json::Value;
    match value {
        Value::Object(map) => {
            let mut entries: Vec<_> = map.iter().collect();
            entries.sort_by(|(a, _), (b, _)| {
                a.len()
                    .cmp(&b.len())
                    .then_with(|| a.as_bytes().cmp(b.as_bytes()))
            });
            out.push('{');
            for (i, (key, item)) in entries.into_iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&serde_json::Value::String(key.clone()).to_string());
                out.push_str(": ");
                write_pg_jsonb(item, out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_pg_jsonb(item, out);
            }
            out.push(']');
        }
        scalar => out.push_str(&scalar.to_string()),
    }
}

/// Lower bound for `GET /activity-logs/failed-logins?since_hours=N`. Any
/// positive N is accepted (echobackend has no cap); a window reaching past
/// the representable range simply covers every row.
pub(super) fn failed_logins_since(since_hours: i64) -> chrono::DateTime<Utc> {
    Duration::try_hours(since_hours)
        .and_then(|window| Utc::now().checked_sub_signed(window))
        .filter(|since| since.timestamp() > 0)
        .unwrap_or(DateTime::UNIX_EPOCH)
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
    let since = failed_logins_since(since_hours);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pg_jsonb_text_matches_postgres_output() {
        assert_eq!(
            pg_jsonb_text(&serde_json::json!({"provider": "github"})),
            r#"{"provider": "github"}"#
        );
        // jsonb orders keys by length, then bytewise.
        assert_eq!(
            pg_jsonb_text(&serde_json::json!({
                "revoked_sessions": 3,
                "scope": "all_users",
                "revoked_by": "u1",
            })),
            r#"{"scope": "all_users", "revoked_by": "u1", "revoked_sessions": 3}"#
        );
        assert_eq!(
            pg_jsonb_text(&serde_json::json!({"a": [1, true, null], "b": {}, "c": []})),
            r#"{"a": [1, true, null], "b": {}, "c": []}"#
        );
        assert_eq!(
            pg_jsonb_text(&serde_json::json!({"error": "line\nbreak \"q\""})),
            r#"{"error": "line\nbreak \"q\""}"#
        );
    }

    #[test]
    fn test_failed_logins_since_saturates() {
        let since = failed_logins_since(24);
        let expected = Utc::now() - Duration::hours(24);
        assert!((since - expected).num_seconds().abs() < 5);
        assert_eq!(failed_logins_since(i64::MAX), DateTime::UNIX_EPOCH);
    }
}
