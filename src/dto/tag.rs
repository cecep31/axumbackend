use serde::Deserialize;
use validator::Validate;

#[derive(Deserialize, Validate)]
pub struct CreateTagRequest {
    #[validate(length(min = 1, max = 30))]
    pub name: String,
}

#[derive(Deserialize, Validate)]
pub struct UpdateTagRequest {
    #[validate(length(min = 1, max = 30))]
    pub name: String,
}

#[derive(Deserialize, Validate)]
pub struct TagIdPath {
    #[validate(range(min = 0))]
    pub id: i32,
}
