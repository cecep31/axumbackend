use super::post::Post;
use chrono::{DateTime, FixedOffset, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Serialize, Clone)]
pub struct BookmarkFolderResponse {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub bookmark_count: i64,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
pub struct BookmarkResponse {
    pub id: Uuid,
    pub post_id: Uuid,
    pub user_id: Uuid,
    pub folder_id: Option<Uuid>,
    pub name: Option<String>,
    pub notes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post: Option<Post>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<BookmarkFolderResponse>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
pub struct ToggleBookmarkResponse {
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bookmark: Option<BookmarkResponse>,
}

fn to_utc(value: Option<DateTime<FixedOffset>>) -> Option<DateTime<Utc>> {
    value.map(|dt| dt.with_timezone(&Utc))
}

impl BookmarkFolderResponse {
    pub fn from_entity(
        folder: crate::entities::bookmark_folders::Model,
        bookmark_count: i64,
    ) -> Self {
        Self {
            id: folder.id,
            user_id: folder.user_id,
            name: folder.name,
            description: folder.description,
            bookmark_count,
            created_at: to_utc(folder.created_at),
            updated_at: to_utc(folder.updated_at),
        }
    }
}

impl BookmarkResponse {
    pub fn from_entity(
        bookmark: crate::entities::post_bookmarks::Model,
        post: Option<Post>,
        folder: Option<BookmarkFolderResponse>,
    ) -> Self {
        Self {
            id: bookmark.id,
            post_id: bookmark.post_id,
            user_id: bookmark.user_id,
            folder_id: bookmark.folder_id,
            name: bookmark.name,
            notes: bookmark.notes,
            post,
            folder,
            created_at: to_utc(bookmark.created_at),
            updated_at: to_utc(bookmark.updated_at),
        }
    }
}
