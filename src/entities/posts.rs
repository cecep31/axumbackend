use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "posts")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub created_at: Option<DateTimeWithTimeZone>,
    pub updated_at: Option<DateTimeWithTimeZone>,
    pub deleted_at: Option<DateTimeWithTimeZone>,
    pub title: String,
    pub created_by: Uuid,
    pub body: Option<String>,
    pub slug: String,
    pub photo_url: Option<String>,
    pub published: Option<bool>,
    pub published_at: Option<DateTimeWithTimeZone>,
    pub view_count: Option<i64>,
    pub like_count: Option<i64>,
    pub bookmark_count: Option<i64>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::post_bookmarks::Entity")]
    PostBookmarks,
    #[sea_orm(has_many = "super::post_comments::Entity")]
    PostComments,
    #[sea_orm(
        belongs_to = "super::users::Entity",
        from = "Column::CreatedBy",
        to = "super::users::Column::Id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    Users,
    #[sea_orm(has_many = "super::posts_to_tags::Entity")]
    PostsToTags,
    #[sea_orm(has_many = "super::post_likes::Entity")]
    PostLikes,
    #[sea_orm(has_many = "super::post_views::Entity")]
    PostViews,
}

impl Related<super::post_bookmarks::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PostBookmarks.def()
    }
}

impl Related<super::post_comments::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PostComments.def()
    }
}

impl Related<super::users::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Users.def()
    }
}

impl Related<super::post_likes::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PostLikes.def()
    }
}

impl Related<super::post_views::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PostViews.def()
    }
}

impl Related<super::tags::Entity> for Entity {
    fn to() -> RelationDef {
        super::posts_to_tags::Relation::Tags.def()
    }

    fn via() -> Option<RelationDef> {
        Some(Relation::PostsToTags.def().rev())
    }
}

impl ActiveModelBehavior for ActiveModel {}
