use garde::Validate;
use serde::Deserialize;
use uuid::Uuid;

/// `/api/bookmarks/{id}`. Taken raw like echobackend's `c.Param(..)`: the
/// handler parses it after the body, so a malformed id gets echobackend's
/// status (404 "Bookmark not found", or 400 "invalid post ID format" on the
/// toggle route) instead of a 400 path rejection.
#[derive(Deserialize, Validate)]
pub struct BookmarkPath {
    #[garde(skip)]
    pub id: String,
}

/// `/api/bookmarks/folders/{folder_id}`, raw for the same reason: a malformed
/// id is a 404 "Bookmark folder not found".
#[derive(Deserialize, Validate)]
pub struct FolderIdPath {
    #[garde(skip)]
    pub folder_id: String,
}

/// echobackend `omitempty,uuid`: empty passes, anything else must be a UUID.
fn optional_uuid<T: AsRef<str>>(value: &T, _: &()) -> garde::Result {
    let value = value.as_ref();
    if value.is_empty() || Uuid::parse_str(value).is_ok() {
        Ok(())
    } else {
        Err(garde::Error::new("uuid"))
    }
}

/// Optional body: an empty request body is treated as `{}` (see
/// `http::extract::VJsonOrDefault`).
#[derive(Deserialize, Validate, Default)]
pub struct ToggleBookmarkRequest {
    /// Kept as a string so a malformed id fails validation (`422`, tag
    /// `uuid`) like echobackend instead of body parsing (`400`).
    #[garde(inner(custom(optional_uuid)))]
    pub folder_id: Option<String>,
    #[garde(length(chars, max = 255))]
    pub name: Option<String>,
    #[garde(length(chars, max = 2000))]
    pub notes: Option<String>,
}

impl ToggleBookmarkRequest {
    /// The validated `folder_id`; empty means no folder.
    pub fn folder_uuid(&self) -> Option<Uuid> {
        self.folder_id
            .as_deref()
            .filter(|id| !id.is_empty())
            .and_then(|id| Uuid::parse_str(id).ok())
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn toggle(folder_id: Option<&str>) -> ToggleBookmarkRequest {
        ToggleBookmarkRequest {
            folder_id: folder_id.map(str::to_string),
            ..Default::default()
        }
    }

    #[test]
    fn toggle_folder_id_is_omitempty_uuid() {
        assert!(toggle(None).validate().is_ok());
        assert!(toggle(Some("")).validate().is_ok());
        assert!(toggle(Some("not-a-uuid")).validate().is_err());

        let id = "0190a000-0000-7000-8000-000000000001";
        assert!(toggle(Some(id)).validate().is_ok());
        assert_eq!(toggle(Some(id)).folder_uuid(), Uuid::parse_str(id).ok());
        assert_eq!(toggle(Some("")).folder_uuid(), None);
    }
}
