use garde::Validate;
use serde::Deserialize;
use uuid::Uuid;

#[derive(Deserialize, Validate)]
pub struct CommentRequest {
    #[garde(length(chars, min = 1, max = 1000))]
    pub text: String,
}

#[derive(Deserialize, Validate)]
pub struct CommentPath {
    #[garde(skip)]
    pub id: Uuid,
    #[garde(skip)]
    pub comment_id: Uuid,
}
