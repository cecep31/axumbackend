use crate::dto::validation::required;
use garde::Validate;
use serde::Deserialize;

/// echobackend `CreateCommentRequest`: `text` `required,min=1,max=1000`. A
/// missing `text` defaults to empty so it is a 422 `required`.
#[derive(Deserialize, Validate, Default)]
#[serde(default)]
pub struct CommentRequest {
    #[garde(custom(required), length(chars, min = 1, max = 1000))]
    pub text: String,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::validation::failing_tags;

    #[test]
    fn comment_text_is_required() {
        let missing: CommentRequest = serde_json::from_str("{}").unwrap();
        assert_eq!(
            failing_tags(&missing),
            vec![("Text".to_string(), "required".to_string())]
        );
        let long = CommentRequest {
            text: "x".repeat(1001),
        };
        assert_eq!(
            failing_tags(&long),
            vec![("Text".to_string(), "max".to_string())]
        );
        assert!(failing_tags(&CommentRequest { text: "ok".into() }).is_empty());
    }
}
