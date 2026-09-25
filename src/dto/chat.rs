use garde::Validate;
use serde::Deserialize;

/// Mirrors echobackend's `free_model` validator (`pkg/validator/validator.go`):
/// a model ID must be `openrouter/free` or end with `:free`.
fn validate_free_model<T: AsRef<str>>(model: &T, _: &()) -> garde::Result {
    let trimmed = model.as_ref().trim();
    if trimmed.eq_ignore_ascii_case("openrouter/free") || trimmed.ends_with(":free") {
        Ok(())
    } else {
        Err(garde::Error::new("free_model"))
    }
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateChatConversationRequest {
    #[garde(length(chars, min = 1, max = 255))]
    pub title: String,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateChatConversationStreamRequest {
    #[garde(length(chars, min = 1, max = 255))]
    pub title: Option<String>,
    #[garde(length(chars, min = 1, max = 10000))]
    pub content: String,
    #[garde(length(chars, max = 100), inner(custom(validate_free_model)))]
    pub model: Option<String>,
    #[garde(range(min = 0.0, max = 2.0))]
    pub temperature: Option<f64>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpdateChatConversationRequest {
    #[garde(length(chars, max = 255))]
    pub title: Option<String>,
    #[garde(skip)]
    pub is_pinned: Option<bool>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateChatMessageRequest {
    #[garde(length(chars, min = 1, max = 10000))]
    pub content: String,
    #[garde(length(chars, max = 20))]
    pub role: Option<String>,
    #[garde(length(chars, max = 100), inner(custom(validate_free_model)))]
    pub model: Option<String>,
    #[garde(range(min = 0.0, max = 2.0))]
    pub temperature: Option<f64>,
}
