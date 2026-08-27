use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "users")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub created_at: Option<DateTimeWithTimeZone>,
    pub updated_at: Option<DateTimeWithTimeZone>,
    pub deleted_at: Option<DateTimeWithTimeZone>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub email: String,
    pub password: Option<String>,
    pub image: Option<String>,
    pub is_super_admin: Option<bool>,
    pub username: Option<String>,
    pub github_id: Option<i64>,
    pub last_logged_at: Option<DateTimeWithTimeZone>,
    pub followers_count: Option<i64>,
    pub following_count: Option<i64>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::bookmark_folders::Entity")]
    BookmarkFolders,
    #[sea_orm(has_many = "super::post_bookmarks::Entity")]
    PostBookmarks,
    #[sea_orm(has_many = "super::post_comments::Entity")]
    PostComments,
    #[sea_orm(has_many = "super::post_likes::Entity")]
    PostLikes,
    #[sea_orm(has_many = "super::post_views::Entity")]
    PostViews,
    #[sea_orm(has_many = "super::posts::Entity")]
    Posts,
    #[sea_orm(has_one = "super::profiles::Entity")]
    Profile,
    #[sea_orm(has_many = "super::sessions::Entity")]
    Sessions,
}

impl Related<super::bookmark_folders::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::BookmarkFolders.def()
    }
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

impl Related<super::posts::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Posts.def()
    }
}

impl Related<super::profiles::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Profile.def()
    }
}

impl Related<super::sessions::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Sessions.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
