use serde::{Deserialize, Serialize};
use uuid::Uuid;
use super::user::User;

#[derive(Serialize, Deserialize)]
pub struct Post {
    pub id: Uuid,
    pub title: String,
    pub body: String,
    pub created_by: Uuid,
    pub slug: String,
    pub creator: User,
}
