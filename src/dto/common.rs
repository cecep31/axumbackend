use crate::dto::validation::parse_pagination;
use garde::Validate;
use serde::Deserialize;
use uuid::Uuid;

/// Lenient pagination query, mirroring echobackend's `ParsePaginationParams`:
/// invalid or out-of-range values are silently clamped/defaulted rather than
/// rejected with a `422`.
#[derive(Deserialize, Validate)]
pub struct PaginationQuery {
    #[garde(skip)]
    pub offset: Option<String>,
    #[garde(skip)]
    pub limit: Option<String>,
}

impl PaginationQuery {
    /// Returns `(limit, offset)`, mirroring `ParsePaginationParams(defaultLimit)`.
    pub fn resolve(&self, default_limit: i64) -> (i64, i64) {
        parse_pagination(self.offset.as_deref(), self.limit.as_deref(), default_limit)
    }
}

#[derive(Deserialize, Validate)]
/// Any non-empty segment, like echobackend's `c.Param("username")`: stored
/// usernames are only length-checked (3..30, any charset), so e.g.
/// `john.doe` must stay reachable; unknown names are a 404.
pub struct UsernamePath {
    #[garde(length(chars, min = 1))]
    pub username: String,
}

#[derive(Deserialize, Validate)]
pub struct PostIdPath {
    #[garde(skip)]
    pub id: Uuid,
}
