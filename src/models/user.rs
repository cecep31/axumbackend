use chrono::{DateTime, FixedOffset, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone)]
pub struct User {
    pub id: Uuid,
    pub username: Option<String>,
    pub image: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Profile {
    pub id: i32,
    pub user_id: Uuid,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    pub bio: Option<String>,
    pub website: Option<String>,
    pub phone: Option<String>,
    pub location: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct UserResponse {
    pub id: Uuid,
    pub email: String,
    pub name: String,
    pub username: Option<String>,
    pub image: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub followers_count: i64,
    pub following_count: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_following: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_super_admin: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<Profile>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<DateTime<Utc>>,
}

fn to_utc(value: Option<DateTime<FixedOffset>>) -> Option<DateTime<Utc>> {
    value.map(|dt| dt.with_timezone(&Utc))
}

fn full_name(first_name: &Option<String>, last_name: &Option<String>) -> String {
    match (first_name, last_name) {
        (Some(first), Some(last)) => format!("{} {}", first, last),
        (Some(first), None) => first.clone(),
        (None, Some(last)) => last.clone(),
        (None, None) => String::new(),
    }
}

impl From<crate::entities::users::Model> for User {
    fn from(model: crate::entities::users::Model) -> Self {
        Self {
            id: model.id,
            username: model.username,
            image: model.image,
        }
    }
}

impl From<crate::entities::profiles::Model> for Profile {
    fn from(model: crate::entities::profiles::Model) -> Self {
        Self {
            id: model.id,
            user_id: model.user_id,
            created_at: to_utc(model.created_at),
            updated_at: to_utc(model.updated_at),
            bio: model.bio,
            website: model.website,
            phone: model.phone,
            location: model.location,
        }
    }
}

impl UserResponse {
    pub fn from_entity(
        user: crate::entities::users::Model,
        profile: Option<crate::entities::profiles::Model>,
        is_following: Option<bool>,
    ) -> Self {
        let name = full_name(&user.first_name, &user.last_name);
        Self {
            id: user.id,
            email: user.email,
            name,
            username: user.username,
            image: user.image,
            first_name: user.first_name,
            last_name: user.last_name,
            followers_count: user.followers_count.unwrap_or_default(),
            following_count: user.following_count.unwrap_or_default(),
            is_following,
            is_super_admin: user.is_super_admin,
            profile: profile.map(Into::into),
            created_at: to_utc(user.created_at),
            updated_at: to_utc(user.updated_at),
            deleted_at: to_utc(user.deleted_at),
        }
    }
}
