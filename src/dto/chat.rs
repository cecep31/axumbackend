use serde::Deserialize;
use validator::{Validate, ValidationError};

/// Mirrors echobackend's `free_model` validator (`pkg/validator/validator.go`):
/// a model ID must be `openrouter/free` or end with `:free`.
fn validate_free_model(model: &str) -> Result<(), ValidationError> {
    let trimmed = model.trim();
    if trimmed.eq_ignore_ascii_case("openrouter/free") || trimmed.ends_with(":free") {
        Ok(())
    } else {
        Err(ValidationError::new("free_model"))
    }
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateChatConversationRequest {
    #[validate(length(min = 1, max = 255))]
    pub title: String,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateChatConversationStreamRequest {
    #[validate(length(min = 1, max = 255))]
    pub title: Option<String>,
    #[validate(length(min = 1, max = 10000))]
    pub content: String,
    #[validate(length(max = 100), custom(function = "validate_free_model"))]
    pub model: Option<String>,
    #[validate(range(min = 0.0, max = 2.0))]
    pub temperature: Option<f64>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpdateChatConversationRequest {
    #[validate(length(max = 255))]
    pub title: Option<String>,
    pub is_pinned: Option<bool>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateChatMessageRequest {
    #[validate(length(min = 1, max = 10000))]
    pub content: String,
    #[validate(length(max = 20))]
    pub role: Option<String>,
    #[validate(length(max = 100), custom(function = "validate_free_model"))]
    pub model: Option<String>,
    #[validate(range(min = 0.0, max = 2.0))]
    pub temperature: Option<f64>,
}
