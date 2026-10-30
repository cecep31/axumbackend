use garde::Validate;
use serde::Deserialize;

/// Mirrors echobackend's `free_model` validator (`pkg/validator/validator.go`):
/// a model ID must be `openrouter/free` or end with `:free`.
/// An empty string is treated as absent (`omitempty`); the check runs on the
/// raw value, so a whitespace-only model is still rejected.
fn validate_free_model<T: AsRef<str>>(model: &T, _: &()) -> garde::Result {
    if model.as_ref().is_empty() {
        return Ok(());
    }
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
    /// echobackend `omitempty,min=1,max=255`: "" counts as absent (the
    /// conversation is auto-titled).
    #[garde(length(chars, max = 255))]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_title_and_model_count_as_absent() {
        let req = CreateChatConversationStreamRequest {
            title: Some(String::new()),
            content: "hello".into(),
            model: Some(String::new()),
            temperature: None,
        };
        assert!(req.validate().is_ok());

        let blank_model = CreateChatMessageRequest {
            content: "hello".into(),
            role: None,
            model: Some("  ".into()),
            temperature: None,
        };
        assert!(blank_model.validate().is_err());
    }
}
