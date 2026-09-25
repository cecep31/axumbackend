use sea_orm::entity::prelude::*;

/// Guilds are hard-deleted: deleting one cascades to its members, channels and
/// messages through the foreign keys. `member_count` is maintained by a
/// database trigger on `guild_members`; never write it by hand.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "guilds")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub owner_id: Uuid,
    pub name: String,
    #[sea_orm(unique)]
    pub slug: String,
    pub description: Option<String>,
    pub avatar_url: Option<String>,
    pub is_public: bool,
    pub member_count: i64,
    pub created_at: Option<DateTimeWithTimeZone>,
    pub updated_at: Option<DateTimeWithTimeZone>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::users::Entity",
        from = "Column::OwnerId",
        to = "super::users::Column::Id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    Users,
    #[sea_orm(has_many = "super::guild_members::Entity")]
    GuildMembers,
    #[sea_orm(has_many = "super::guild_channels::Entity")]
    GuildChannels,
}

impl Related<super::users::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Users.def()
    }
}

impl Related<super::guild_members::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::GuildMembers.def()
    }
}

impl Related<super::guild_channels::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::GuildChannels.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
