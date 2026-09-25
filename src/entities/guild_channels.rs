use sea_orm::entity::prelude::*;

/// A text channel inside a guild. Hard-deleted; deleting it cascades to its
/// messages.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "guild_channels")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub guild_id: Uuid,
    pub name: String,
    pub topic: Option<String>,
    pub position: i32,
    pub created_by: Option<Uuid>,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
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
    #[sea_orm(has_many = "super::guild_channel_messages::Entity")]
    GuildChannelMessages,
}

impl Related<super::guilds::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Guilds.def()
    }
}

impl Related<super::guild_channel_messages::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::GuildChannelMessages.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
