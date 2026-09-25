use sea_orm::entity::prelude::*;

/// Rows are hard-deleted on leave so the `member_count` trigger stays correct.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "guild_members")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub guild_id: Uuid,
    pub user_id: Uuid,
    pub role: String,
    pub created_at: Option<DateTimeWithTimeZone>,
    pub updated_at: Option<DateTimeWithTimeZone>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::guilds::Entity",
        from = "Column::GuildId",
        to = "super::guilds::Column::Id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    Guilds,
    #[sea_orm(
        belongs_to = "super::users::Entity",
        from = "Column::UserId",
        to = "super::users::Column::Id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    Users,
}

impl Related<super::guilds::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Guilds.def()
    }
}

impl Related<super::users::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Users.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
