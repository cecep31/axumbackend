use garde::Validate;
use serde::Deserialize;

#[derive(Deserialize, Validate)]
pub struct CreateTagRequest {
    #[garde(length(chars, min = 1, max = 30))]
    pub name: String,
}

#[derive(Deserialize, Validate)]
pub struct UpdateTagRequest {
    #[garde(length(chars, min = 1, max = 30))]
    pub name: String,
}

#[derive(Deserialize, Validate)]
pub struct TagIdPath {
    #[garde(range(min = 0))]
    pub id: i32,
}
