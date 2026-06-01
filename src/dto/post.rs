use crate::dto::validation::{SLUG_RE, TAG_RE, USERNAME_RE};
use crate::services;
use serde::Deserialize;
use validator::Validate;

#[derive(Deserialize, Validate)]
pub struct CreatePostRequest {
    #[validate(length(min = 7))]
    pub title: String,
    pub photo_url: Option<String>,
    #[validate(length(min = 7, max = 100), regex(path = *SLUG_RE))]
    pub slug: String,
    #[validate(length(min = 10))]
    pub body: String,
    #[serde(default)]
    pub published: bool,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Deserialize, Validate)]
pub struct UpdatePostRequest {
    #[validate(length(min = 1))]
    pub title: Option<String>,
    pub photo_url: Option<String>,
    #[validate(length(min = 1, max = 100), regex(path = *SLUG_RE))]
    pub slug: Option<String>,
    #[validate(length(min = 1))]
    pub body: Option<String>,
    pub published: Option<bool>,
    pub tags: Option<Vec<String>>,
}

#[derive(Deserialize, Validate)]
pub struct RandomPostQuery {
    #[validate(range(min = 1, max = 100))]
    pub limit: Option<i64>,
}

#[derive(Deserialize, Validate)]
pub struct MyPostsAnalyticsQuery {
    pub start_date: Option<chrono::NaiveDate>,
    pub end_date: Option<chrono::NaiveDate>,
}

#[derive(Deserialize, Validate)]
pub struct MyPostsLikesByMonthQuery {
    #[validate(range(min = 1, max = 24))]
    pub months: Option<i64>,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum OrderDirection {
    Asc,
    Desc,
}

impl From<OrderDirection> for services::post::SortDirection {
    fn from(value: OrderDirection) -> Self {
        match value {
            OrderDirection::Asc => Self::Asc,
            OrderDirection::Desc => Self::Desc,
        }
    }
}

#[derive(Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct PostPaginationQuery {
    #[validate(range(min = 0, max = 10_000))]
    pub offset: Option<i64>,
    #[validate(range(min = 1, max = 100))]
    pub limit: Option<i64>,
    #[validate(length(max = 200))]
    pub search: Option<String>,
    #[serde(alias = "sort_by")]
    pub order_by: Option<String>,
    #[serde(alias = "sort_order")]
    pub order_direction: Option<OrderDirection>,
}

#[derive(Deserialize, Validate)]
pub struct TagPath {
    #[validate(length(min = 1, max = 50), regex(path = *TAG_RE))]
    pub tag: String,
}

#[derive(Deserialize, Validate)]
pub struct PostPath {
    #[validate(length(min = 1, max = 50), regex(path = *USERNAME_RE))]
    pub username: String,
    #[validate(length(min = 1, max = 100), regex(path = *SLUG_RE))]
    pub slug: String,
}

pub fn post_pagination_params(
    query: &PostPaginationQuery,
) -> (
    i64,
    i64,
    Option<&str>,
    Option<&str>,
    Option<services::post::SortDirection>,
) {
    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(10);
    let search = query.search.as_deref();
    let order_by = query.order_by.as_deref();
    let order_direction = query.order_direction.map(Into::into);
    (offset, limit, search, order_by, order_direction)
}
