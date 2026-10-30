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

/// `/api/posts/{id}/comments`: the id is taken raw like echobackend's
/// `c.Param("id")`, so a malformed id is a 404 "post not found" (echobackend's
/// `GetPostByID` failure) instead of a 400 path rejection, and is reported
/// only after the body has been validated.
#[derive(Deserialize, Validate)]
pub struct CommentPostPath {
    #[garde(skip)]
    pub id: String,
}
