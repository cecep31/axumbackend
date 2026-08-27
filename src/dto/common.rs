use crate::dto::validation::{USERNAME_RE, parse_pagination};
use serde::Deserialize;
use uuid::Uuid;
use validator::Validate;

/// Lenient pagination query, mirroring echobackend's `ParsePaginationParams`:
/// invalid or out-of-range values are silently clamped/defaulted rather than
/// rejected with a `422`.
#[derive(Deserialize, Validate)]
pub struct PaginationQuery {
    pub offset: Option<String>,
    pub limit: Option<String>,
}

impl PaginationQuery {
    /// Returns `(limit, offset)`, mirroring `ParsePaginationParams(defaultLimit)`.
    pub fn resolve(&self, default_limit: i64) -> (i64, i64) {
        parse_pagination(self.offset.as_deref(), self.limit.as_deref(), default_limit)
    }
}

#[derive(Deserialize, Validate)]
pub struct UsernamePath {
    #[validate(length(min = 1, max = 50), regex(path = *USERNAME_RE))]
    pub username: String,
}

#[derive(Deserialize, Validate)]
pub struct PostIdPath {
    pub id: Uuid,
}
