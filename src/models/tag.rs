use chrono::{DateTime, FixedOffset, Utc};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
pub struct SitemapTag {
    pub name: String,
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Tag {
    pub id: i32,
    pub name: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct TrendingTag {
    pub id: i32,
    pub name: String,
    pub total_views: i64,
    pub total_likes: i64,
    pub trending_score: i64,
}

fn to_utc(value: Option<DateTime<FixedOffset>>) -> Option<DateTime<Utc>> {
    value.map(|dt| dt.with_timezone(&Utc))
}

impl From<crate::entities::tags::Model> for Tag {
    fn from(model: crate::entities::tags::Model) -> Self {
        Self {
            id: model.id,
            name: model.name,
        }
    }
}

impl From<crate::entities::tags::Model> for SitemapTag {
    fn from(model: crate::entities::tags::Model) -> Self {
        Self {
            name: model.name,
            created_at: to_utc(model.created_at),
        }
    }
}
