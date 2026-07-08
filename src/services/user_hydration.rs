use crate::entities::{profiles, users};
use crate::models::user::{UserResponse, UserView};
use sea_orm::{ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

pub async fn load_user_response_map(
    db: &DatabaseConnection,
    user_ids: impl IntoIterator<Item = Uuid>,
    view: UserView,
) -> Result<HashMap<Uuid, UserResponse>, DbErr> {
    let user_ids: HashSet<Uuid> = user_ids.into_iter().collect();
    if user_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let users = users::Entity::find()
        .filter(users::Column::Id.is_in(user_ids.iter().copied()))
        .filter(users::Column::DeletedAt.is_null())
        .all(db)
        .await?;

    let profiles_by_user_id: HashMap<Uuid, profiles::Model> = profiles::Entity::find()
        .filter(profiles::Column::UserId.is_in(user_ids.iter().copied()))
        .all(db)
        .await?
        .into_iter()
        .map(|profile| (profile.user_id, profile))
        .collect();

    Ok(users
        .into_iter()
        .map(|user| {
            let user_id = user.id;
            let profile = profiles_by_user_id.get(&user_id).cloned();
            (
                user_id,
                UserResponse::from_entity_with_view(user, profile, None, view),
            )
        })
        .collect())
}
