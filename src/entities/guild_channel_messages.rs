use sea_orm::entity::prelude::*;

/// A chat message posted to a guild channel. Hard-deleted; a reply whose
/// parent is deleted keeps `reply_to_id = NULL`. There is no `updated_at`:
/// content is the only mutable column and `edited_at` records that.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "guild_channel_messages")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub channel_id: Uuid,
    pub author_id: Uuid,
    #[sea_orm(column_type = "Text")]
    pub content: String,
    pub reply_to_id: Option<Uuid>,
    pub edited_at: Option<DateTimeWithTimeZone>,
    pub created_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::guild_channels::Entity",
        from = "Column::ChannelId",
        to = "super::guild_channels::Column::Id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    GuildChannels,
    #[sea_orm(
        belongs_to = "super::users::Entity",
        from = "Column::AuthorId",
        to = "super::users::Column::Id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    Users,
}

impl Related<super::guild_channels::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::GuildChannels.def()
    }
}

impl Related<super::users::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Users.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
