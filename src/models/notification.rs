use chrono::{DateTime, FixedOffset, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
pub struct NotificationResponse {
    pub id: Uuid,
    pub user_id: Uuid,
    #[serde(rename = "type")]
    pub notification_type: String,
    pub title: String,
    pub message: Option<String>,
    pub read: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize)]
pub struct UnreadCountResponse {
    pub unread_count: i64,
}

#[derive(Serialize, Deserialize)]
pub struct MarkAllReadResponse {
    pub updated_count: i64,
}

fn to_utc(value: DateTime<FixedOffset>) -> DateTime<Utc> {
    value.with_timezone(&Utc)
}

impl From<crate::entities::notifications::Model> for NotificationResponse {
    fn from(model: crate::entities::notifications::Model) -> Self {
        let data = model
            .data
            .as_deref()
            .and_then(|payload| serde_json::from_str(payload).ok());

        Self {
            id: model.id,
            user_id: model.user_id,
            notification_type: model.r#type,
            title: model.title,
            message: model.message,
            read: model.read,
            data,
            created_at: to_utc(model.created_at),
            updated_at: to_utc(model.updated_at),
        }
    }
}
