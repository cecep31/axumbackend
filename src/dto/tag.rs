use serde::Deserialize;
use validator::Validate;

#[derive(Deserialize, Validate)]
pub struct CreateTagRequest {
    #[validate(length(min = 1, max = 100))]
    pub name: String,
}

#[derive(Deserialize, Validate)]
pub struct UpdateTagRequest {
    #[validate(length(min = 1, max = 100))]
    pub name: String,
}

#[derive(Deserialize, Validate)]
pub struct TrendingTagQuery {
    #[validate(range(min = 1, max = 100))]
    pub limit: Option<i64>,
}

#[derive(Deserialize, Validate)]
pub struct TagIdPath {
    #[validate(range(min = 1))]
    pub id: i32,
}
