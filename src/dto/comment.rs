use serde::Deserialize;
use uuid::Uuid;
use validator::Validate;

#[derive(Deserialize, Validate)]
pub struct CommentRequest {
    #[validate(length(min = 1, max = 1000))]
    pub text: String,
}

#[derive(Deserialize, Validate)]
pub struct CommentPath {
    pub id: Uuid,
    pub comment_id: Uuid,
}
