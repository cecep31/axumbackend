use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "tags")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub name: String,
    pub created_at: Option<DateTimeWithTimeZone>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::posts_to_tags::Entity")]
    PostsToTags,
}

impl Related<super::posts::Entity> for Entity {
    fn to() -> RelationDef {
        super::posts_to_tags::Relation::Posts.def()
    }

    fn via() -> Option<RelationDef> {
        Some(Relation::PostsToTags.def().rev())
    }
}

impl ActiveModelBehavior for ActiveModel {}
