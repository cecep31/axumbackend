use garde::Validate;
use serde::Deserialize;
use uuid::Uuid;

#[derive(Deserialize, Validate)]
pub struct NotificationQuery {
    /// Kept as a raw string and compared against `"true"`, mirroring
    /// echobackend's `c.QueryParam("unread") == "true"` (lenient parsing).
    #[garde(skip)]
    pub unread: Option<String>,
    #[garde(skip)]
    pub limit: Option<String>,
    #[garde(skip)]
    pub offset: Option<String>,
}

impl NotificationQuery {
    /// Returns `(limit, offset)`, mirroring echobackend's
    /// `ParsePaginationParams(c, 20)`.
    pub fn resolve(&self) -> (i64, i64) {
        crate::dto::validation::parse_pagination(self.offset.as_deref(), self.limit.as_deref(), 20)
    }
}

#[derive(Deserialize, Validate)]
pub struct NotificationPath {
    #[garde(skip)]
    pub id: Uuid,
}
