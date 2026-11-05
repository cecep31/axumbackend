use crate::services::article_generation::{
    ArticleLength, ArticleTone, LENGTH_CHOICES, TONE_CHOICES,
};
use garde::Validate;
use serde::Deserialize;

const PROMPT_MIN_CHARS: usize = 3;
const PROMPT_MAX_CHARS: usize = 2000;
const TITLE_MAX_CHARS: usize = 200;

/// `prompt` is required and 3..=2000 characters once trimmed.
fn validate_prompt<T: AsRef<str>>(value: &T, _: &()) -> garde::Result {
    let chars = value.as_ref().trim().chars().count();
    if chars == 0 {
        Err(garde::Error::new("required"))
    } else if chars < PROMPT_MIN_CHARS {
        Err(garde::Error::new(format!("min:{PROMPT_MIN_CHARS}")))
    } else if chars > PROMPT_MAX_CHARS {
        Err(garde::Error::new(format!("max:{PROMPT_MAX_CHARS}")))
    } else {
        Ok(())
    }
}

/// Optional `title`: at most 200 characters once trimmed.
fn validate_title<T: AsRef<str>>(value: &T, _: &()) -> garde::Result {
    if value.as_ref().trim().chars().count() > TITLE_MAX_CHARS {
        Err(garde::Error::new(format!("max:{TITLE_MAX_CHARS}")))
    } else {
        Ok(())
    }
}

/// `omitempty,oneof=...`: empty means "use the default".
fn optional_oneof(value: &str, choices: &[&str]) -> garde::Result {
    if value.is_empty() || choices.contains(&value) {
        Ok(())
    } else {
        Err(garde::Error::new(format!("oneof:{}", choices.join(" "))))
    }
}

fn validate_tone<T: AsRef<str>>(value: &T, _: &()) -> garde::Result {
    optional_oneof(value.as_ref(), &TONE_CHOICES)
}

fn validate_length<T: AsRef<str>>(value: &T, _: &()) -> garde::Result {
    optional_oneof(value.as_ref(), &LENGTH_CHOICES)
}

/// `POST /api/ai/generate-article` body. There is deliberately no model or
/// temperature: generation always uses the configured default model.
#[derive(Debug, Default, Deserialize, Validate)]
#[serde(default)]
pub struct GenerateArticleRequest {
    #[garde(custom(validate_prompt))]
    pub prompt: String,
    #[garde(inner(custom(validate_tone)))]
    pub tone: Option<String>,
    #[garde(inner(custom(validate_length)))]
    pub length: Option<String>,
    #[garde(inner(custom(validate_title)))]
    pub title: Option<String>,
}

impl GenerateArticleRequest {
    /// The requested tone, `informative` when absent or empty.
    pub fn tone(&self) -> ArticleTone {
        self.tone
            .as_deref()
            .and_then(ArticleTone::parse)
            .unwrap_or_default()
    }

    /// The requested length, `medium` when absent or empty.
    pub fn length(&self) -> ArticleLength {
        self.length
            .as_deref()
            .and_then(ArticleLength::parse)
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::validation::failing_tags;

    fn tags(json: &str) -> Vec<(String, String)> {
        failing_tags(&serde_json::from_str::<GenerateArticleRequest>(json).unwrap())
    }

    fn pair(field: &str, tag: &str) -> Vec<(String, String)> {
        vec![(field.to_string(), tag.to_string())]
    }

    #[test]
    fn valid_requests_pass() {
        assert!(tags(r#"{"prompt":"Rust async"}"#).is_empty());
        assert!(
            tags(
                r#"{"prompt":"abc","tone":"casual","length":"short","title":"T","model":"x","temperature":2}"#
            )
            .is_empty()
        );
        assert!(tags(r#"{"prompt":"abc","tone":"","length":"","title":""}"#).is_empty());
        let max = "é".repeat(2000);
        assert!(tags(&format!(r#"{{"prompt":"{max}"}}"#)).is_empty());
    }

    #[test]
    fn prompt_is_required_and_bounded_after_trim() {
        assert_eq!(tags("{}"), pair("Prompt", "required"));
        assert_eq!(tags(r#"{"prompt":"   "}"#), pair("Prompt", "required"));
        assert_eq!(tags(r#"{"prompt":"  ab  "}"#), pair("Prompt", "min"));
        let long = "a".repeat(2001);
        assert_eq!(
            tags(&format!(r#"{{"prompt":"{long}"}}"#)),
            pair("Prompt", "max")
        );
    }

    #[test]
    fn tone_and_length_must_be_known_choices() {
        assert_eq!(
            tags(r#"{"prompt":"abc","tone":"angry"}"#),
            pair("Tone", "oneof")
        );
        assert_eq!(
            tags(r#"{"prompt":"abc","length":"huge"}"#),
            pair("Length", "oneof")
        );
    }

    #[test]
    fn title_is_capped_at_200_chars() {
        let ok = "t".repeat(200);
        assert!(tags(&format!(r#"{{"prompt":"abc","title":"{ok}"}}"#)).is_empty());
        let long = "t".repeat(201);
        assert_eq!(
            tags(&format!(r#"{{"prompt":"abc","title":"{long}"}}"#)),
            pair("Title", "max")
        );
    }
}
