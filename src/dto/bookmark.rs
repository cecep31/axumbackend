use garde::Validate;
use serde::Deserialize;
use uuid::Uuid;

#[derive(Deserialize, Validate)]
pub struct BookmarkPath {
    #[garde(skip)]
    pub id: Uuid,
}

#[derive(Deserialize, Validate)]
pub struct FolderIdPath {
    #[garde(skip)]
    pub folder_id: Uuid,
}

#[derive(Deserialize, Validate)]
pub struct ToggleBookmarkRequest {
    #[garde(skip)]
    pub folder_id: Option<Uuid>,
    #[garde(length(chars, max = 255))]
    pub name: Option<String>,
    #[garde(length(chars, max = 2000))]
    pub notes: Option<String>,
}

#[derive(Deserialize, Validate)]
pub struct UpdateBookmarkRequest {
    #[garde(length(chars, max = 255))]
    pub name: Option<String>,
    #[garde(length(chars, max = 2000))]
    pub notes: Option<String>,
}

#[derive(Deserialize, Validate)]
pub struct MoveBookmarkRequest {
    #[garde(skip)]
    pub folder_id: Option<Uuid>,
}

#[derive(Deserialize, Validate)]
pub struct CreateBookmarkFolderRequest {
    #[garde(length(chars, min = 1, max = 100))]
    pub name: String,
    #[garde(length(chars, max = 1000))]
    pub description: Option<String>,
}

#[derive(Deserialize, Validate)]
pub struct UpdateBookmarkFolderRequest {
    #[garde(length(chars, min = 1, max = 100))]
    pub name: Option<String>,
    #[garde(length(chars, max = 1000))]
    pub description: Option<String>,
}

#[derive(Deserialize, Validate)]
pub struct BookmarkQuery {
    #[garde(skip)]
    pub limit: Option<String>,
    #[garde(skip)]
    pub offset: Option<String>,
    #[garde(skip)]
    pub folder_id: Option<String>,
}

impl BookmarkQuery {
    /// Returns `(limit, offset)`, mirroring echobackend's
    /// `ParsePaginationParams(c, 50)`.
    pub fn resolve(&self) -> (i64, i64) {
        crate::dto::validation::parse_pagination(self.offset.as_deref(), self.limit.as_deref(), 50)
    }
}
