use crate::dto::validation::required;
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

/// echobackend `gte=0,lte=2` on `temperature`: tags `gte` / `lte` (garde's
/// `range` would report `min` / `max`).
fn validate_temperature(value: &f64, _: &()) -> garde::Result {
    if *value < 0.0 {
        Err(garde::Error::new("gte"))
    } else if *value > 2.0 {
        Err(garde::Error::new("lte"))
    } else {
        Ok(())
    }
}

/// Bodies default missing fields so echobackend's `required` is a 422 tag
/// `required` (missing or empty), not a body-parse 400.
#[derive(Debug, Default, Deserialize, Validate)]
#[serde(default)]
pub struct CreateChatConversationRequest {
    #[garde(custom(required), length(chars, max = 255))]
    pub title: String,
}

#[derive(Debug, Default, Deserialize, Validate)]
#[serde(default)]
pub struct CreateChatConversationStreamRequest {
    /// echobackend `*string` with `omitempty,min=1,max=255`: absent (or
    /// `null`) means auto-titled, but `""` is still validated and fails `min`.
    #[garde(length(chars, min = 1, max = 255))]
    pub title: Option<String>,
    #[garde(custom(required), length(chars, min = 1, max = 10000))]
    pub content: String,
    #[garde(length(chars, max = 100), inner(custom(validate_free_model)))]
    pub model: Option<String>,
    #[garde(inner(custom(validate_temperature)))]
    pub temperature: Option<f64>,
}

#[derive(Debug, Default, Deserialize, Validate)]
pub struct UpdateChatConversationRequest {
    #[garde(length(chars, max = 255))]
    pub title: Option<String>,
    #[garde(skip)]
    pub is_pinned: Option<bool>,
}

#[derive(Debug, Default, Deserialize, Validate)]
#[serde(default)]
pub struct CreateChatMessageRequest {
    #[garde(custom(required), length(chars, min = 1, max = 10000))]
    pub content: String,
    #[garde(length(chars, max = 20))]
    pub role: Option<String>,
    #[garde(length(chars, max = 100), inner(custom(validate_free_model)))]
    pub model: Option<String>,
    #[garde(inner(custom(validate_temperature)))]
    pub temperature: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::validation::failing_tags;

    fn tags(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(f, t)| (f.to_string(), t.to_string()))
            .collect()
    }

    #[test]
    fn empty_stream_title_fails_min_but_empty_model_is_absent() {
        let req = CreateChatConversationStreamRequest {
            title: Some(String::new()),
            content: "hello".into(),
            model: Some(String::new()),
            temperature: None,
        };
        assert_eq!(failing_tags(&req), tags(&[("Title", "min")]));

        let untitled = CreateChatConversationStreamRequest {
            title: None,
            content: "hello".into(),
            model: Some(String::new()),
            temperature: None,
        };
        assert!(failing_tags(&untitled).is_empty());

        let blank_model = CreateChatMessageRequest {
            content: "hello".into(),
            role: None,
            model: Some("  ".into()),
            temperature: None,
        };
        assert!(blank_model.validate().is_err());
    }

    #[test]
    fn missing_or_empty_required_fields_report_required() {
        let conversation: CreateChatConversationRequest = serde_json::from_str("{}").unwrap();
        assert_eq!(failing_tags(&conversation), tags(&[("Title", "required")]));

        let stream: CreateChatConversationStreamRequest =
            serde_json::from_str(r#"{"content":""}"#).unwrap();
        assert_eq!(failing_tags(&stream), tags(&[("Content", "required")]));

        let message: CreateChatMessageRequest = serde_json::from_str("{}").unwrap();
        assert_eq!(failing_tags(&message), tags(&[("Content", "required")]));
    }

    #[test]
    fn temperature_reports_gte_and_lte() {
        let message = |temperature: f64| CreateChatMessageRequest {
            content: "hi".into(),
            temperature: Some(temperature),
            ..Default::default()
        };
        assert_eq!(
            failing_tags(&message(-0.5)),
            tags(&[("Temperature", "gte")])
        );
        assert_eq!(failing_tags(&message(2.5)), tags(&[("Temperature", "lte")]));
        assert!(failing_tags(&message(0.0)).is_empty());
        assert!(failing_tags(&message(2.0)).is_empty());
    }
}
