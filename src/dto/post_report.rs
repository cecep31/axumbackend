use crate::services::post_report::{ACTION_CHOICES, REASON_CHOICES};
use garde::Validate;
use serde::Deserialize;

/// echobackend `required,oneof=<choices>`: an empty value is `required`, an
/// unknown one `oneof` (rendered "<Field> must be one of [<choices>]").
fn required_oneof(value: &str, choices: &[&str]) -> garde::Result {
    if value.is_empty() {
        return Err(garde::Error::new("required"));
    }
    if choices.contains(&value) {
        Ok(())
    } else {
        Err(garde::Error::new(format!("oneof:{}", choices.join(" "))))
    }
}

fn report_reason<T: AsRef<str>>(value: &T, _: &()) -> garde::Result {
    required_oneof(value.as_ref(), &REASON_CHOICES)
}

fn moderation_action<T: AsRef<str>>(value: &T, _: &()) -> garde::Result {
    required_oneof(value.as_ref(), &ACTION_CHOICES)
}

/// `/api/posts/{id}/reports` and `/api/posts/{id}/moderation`: the id is
/// taken raw like echobackend's `c.Param("id")` and checked by the handler
/// (after the body is validated), so a malformed id is a 400 "invalid post
/// ID format" rather than a path rejection.
#[derive(Deserialize, Validate)]
pub struct ReportPostPath {
    #[garde(skip)]
    pub id: String,
}

/// echobackend `CreatePostReportRequest`. A missing `reason` reaches the
/// validator (`#[serde(default)]`) instead of failing JSON decoding.
#[derive(Deserialize, Validate, Default)]
pub struct CreatePostReportRequest {
    #[serde(default)]
    #[garde(custom(report_reason))]
    pub reason: String,
    #[garde(inner(length(chars, max = 1000)))]
    pub details: Option<String>,
}

/// echobackend `ModeratePostRequest`.
#[derive(Deserialize, Validate, Default)]
pub struct ModeratePostRequest {
    #[serde(default)]
    #[garde(custom(moderation_action))]
    pub action: String,
    #[garde(inner(length(chars, max = 1000)))]
    pub note: Option<String>,
}

#[derive(Deserialize, Validate)]
pub struct ReportStatusQuery {
    #[garde(skip)]
    pub status: Option<String>,
    #[garde(skip)]
    pub limit: Option<String>,
    #[garde(skip)]
    pub offset: Option<String>,
}

impl ReportStatusQuery {
    pub fn status_or_default(&self) -> String {
        let s = self.status.as_deref().unwrap_or("").trim();
        if s.is_empty() {
            "pending".to_string()
        } else {
            s.to_string()
        }
    }

    /// echobackend `ParsePaginationParams(default)`: non-numeric or `<= 0`
    /// gives `default`, capped at 100.
    pub fn limit_or(&self, default: i64) -> u64 {
        crate::dto::validation::parse_pagination(None, self.limit.as_deref(), default).0 as u64
    }

    /// Non-numeric or negative gives 0.
    pub fn offset_or(&self) -> u64 {
        crate::dto::validation::parse_pagination(self.offset.as_deref(), None, 1).1 as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(report: garde::Report) -> Vec<String> {
        report
            .iter()
            .map(|(path, e)| format!("{path}={}", e.message()))
            .collect()
    }

    #[test]
    fn reason_is_required_then_oneof() {
        let empty = CreatePostReportRequest::default();
        assert_eq!(tags(empty.validate().unwrap_err()), ["reason=required"]);

        let unknown = CreatePostReportRequest {
            reason: "foo".into(),
            details: None,
        };
        assert_eq!(
            tags(unknown.validate().unwrap_err()),
            ["reason=oneof:spam harassment hate nsfw misinformation other"]
        );

        let ok = CreatePostReportRequest {
            reason: "spam".into(),
            details: Some(String::new()),
        };
        assert!(ok.validate().is_ok());
    }

    #[test]
    fn action_is_required_then_oneof() {
        let missing: ModeratePostRequest = serde_json::from_str("{}").unwrap();
        assert_eq!(tags(missing.validate().unwrap_err()), ["action=required"]);

        let unknown = ModeratePostRequest {
            action: "delete".into(),
            note: None,
        };
        assert_eq!(
            tags(unknown.validate().unwrap_err()),
            ["action=oneof:hide unhide dismiss"]
        );
    }
}
