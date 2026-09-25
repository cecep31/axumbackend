//! Validated request extractors.
//!
//! Replaces `axum_valid::Valid` so extraction/validation failures use the same
//! JSON envelope as echobackend:
//! - body/query/path parse failures -> `400` (`success: false`)
//! - validation failures -> `422` with `errors: [{field, message, tag}]`,
//!   mirroring echobackend's `response.FromValidateError` envelope.

use crate::error::AppError;
use axum::{
    Json,
    extract::{FromRequest, FromRequestParts, Path, Query, Request},
    http::request::Parts,
};
use garde::i18n::{InvalidCreditCard, InvalidEmail, InvalidPhoneNumber, InvalidUrl, IpKind};
use garde::{I18n, Report, Validate, with_i18n};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::borrow::Cow;
use std::fmt::Display;

/// A single field validation failure, mirroring echobackend's
/// `validator.ValidationError` (`{field, message, value?, tag?}`).
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct FieldError {
    pub field: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
}

/// JSON body extractor with `garde` support (replaces `Valid<Json<T>>`).
pub struct VJson<T>(pub T);

impl<S, T> FromRequest<S> for VJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned + Validate<Context = ()>,
{
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let Json(value) = Json::<T>::from_request(req, state)
            .await
            .map_err(|rejection| {
                AppError::BadRequest(format!("Invalid request body: {}", rejection.body_text()))
            })?;
        validate(&value)?;
        Ok(VJson(value))
    }
}

/// Query extractor with `garde` support (replaces `Valid<Query<T>>`).
pub struct VQuery<T>(pub T);

impl<S, T> FromRequestParts<S> for VQuery<T>
where
    S: Send + Sync,
    T: DeserializeOwned + Validate<Context = ()>,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let Query(value) =
            Query::<T>::from_request_parts(parts, state)
                .await
                .map_err(|rejection| {
                    AppError::BadRequest(format!(
                        "Invalid query parameters: {}",
                        rejection.body_text()
                    ))
                })?;
        validate(&value)?;
        Ok(VQuery(value))
    }
}

/// Path extractor with `garde` support (replaces `Valid<Path<T>>`).
pub struct VPath<T>(pub T);

impl<S, T> FromRequestParts<S> for VPath<T>
where
    S: Send + Sync,
    T: DeserializeOwned + Validate<Context = ()> + Send,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let Path(value) =
            Path::<T>::from_request_parts(parts, state)
                .await
                .map_err(|rejection| {
                    AppError::BadRequest(format!(
                        "Invalid path parameters: {}",
                        rejection.body_text()
                    ))
                })?;
        validate(&value)?;
        Ok(VPath(value))
    }
}

/// Validates `value`, mapping failures to the `422` envelope.
pub fn validate<T: Validate<Context = ()>>(value: &T) -> Result<(), AppError> {
    with_i18n(TagI18n, || value.validate()).map_err(validation_error)
}

/// garde only reports a message per failure, so validation runs under this
/// `I18n`, which emits machine-readable `tag[:param]` tokens that
/// `to_field_error` renders as echobackend-style messages. Custom rules
/// report their tag directly via `garde::Error::new("<tag>")`.
struct TagI18n;

impl I18n for TagI18n {
    fn length_lower_than(&self, min: usize) -> Cow<'static, str> {
        format!("len_min:{min}").into()
    }

    fn length_greater_than(&self, max: usize) -> Cow<'static, str> {
        format!("len_max:{max}").into()
    }

    fn range_lower_than(&self, min: &dyn Display) -> Cow<'static, str> {
        format!("min:{min}").into()
    }

    fn range_greater_than(&self, max: &dyn Display) -> Cow<'static, str> {
        format!("max:{max}").into()
    }

    fn credit_card_invalid(&self, _: InvalidCreditCard) -> Cow<'static, str> {
        "credit_card".into()
    }

    fn pattern_no_match(&self, _: &dyn Display) -> Cow<'static, str> {
        "regex".into()
    }

    fn contains_missing(&self, _: &dyn Display) -> Cow<'static, str> {
        "contains".into()
    }

    fn url_invalid(&self, _: InvalidUrl) -> Cow<'static, str> {
        "url".into()
    }

    fn prefix_missing(&self, _: &dyn Display) -> Cow<'static, str> {
        "prefix".into()
    }

    fn suffix_missing(&self, _: &dyn Display) -> Cow<'static, str> {
        "suffix".into()
    }

    fn phone_number_invalid(&self, _: InvalidPhoneNumber) -> Cow<'static, str> {
        "phone_number".into()
    }

    fn ip_invalid(&self, _: IpKind) -> Cow<'static, str> {
        "ip".into()
    }

    fn matches_field_mismatch(&self, _: &dyn Display) -> Cow<'static, str> {
        "matches".into()
    }

    fn email_invalid(&self, _: InvalidEmail) -> Cow<'static, str> {
        "email".into()
    }

    fn ascii_invalid(&self) -> Cow<'static, str> {
        "ascii".into()
    }

    fn alphanumeric_invalid(&self) -> Cow<'static, str> {
        "alphanumeric".into()
    }

    fn required_not_set(&self) -> Cow<'static, str> {
        "required".into()
    }
}

fn validation_error(report: Report) -> AppError {
    let field_errors: Vec<FieldError> = report
        .iter()
        .map(|(path, error)| to_field_error(&path.to_string(), error.message()))
        .collect();
    // echobackend surfaces the first field error message in the top-level
    // `error` string (`ValidationErrors.Error()`).
    let first_message = field_errors
        .first()
        .map(|error| error.message.clone())
        .unwrap_or_else(|| "validation failed".to_string());
    AppError::UnprocessableEntity {
        error: first_message,
        errors: field_errors,
    }
}

fn to_field_error(field: &str, token: &str) -> FieldError {
    let name = readable_field_name(field);
    let (code, param) = token.split_once(':').unwrap_or((token, ""));
    let (message, tag) = match code {
        "required" => (format!("{name} is required"), "required"),
        "email" => (format!("{name} must be a valid email address"), "email"),
        "len_min" => (
            format!("{name} must be at least {param} characters long"),
            "min",
        ),
        "len_max" => (format!("{name} must not exceed {param} characters"), "max"),
        "min" => (format!("{name} must be at least {param}"), "min"),
        "max" => (format!("{name} must not exceed {param}"), "max"),
        "free_model" => (
            format!("{name} must be a free OpenRouter model (use :free suffix or openrouter/free)"),
            "free_model",
        ),
        code => (format!("{name} failed validation for tag {code}"), code),
    };
    FieldError {
        field: field.to_string(),
        message,
        tag: Some(tag.to_string()),
    }
}

/// `snake_case`/`camelCase` -> `"Title case"` readable field names, mirroring
/// echobackend's `toReadableFieldName`.
fn readable_field_name(field: &str) -> String {
    let spaced = field.replace('_', " ");
    let mut chars = spaced.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => spaced,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(garde::Validate)]
    struct Sample {
        #[garde(email)]
        email: String,
        #[garde(length(chars, min = 8, max = 12))]
        password: String,
        #[garde(range(min = 1, max = 12))]
        month: i32,
    }

    fn sample() -> Sample {
        Sample {
            email: "a@b.co".into(),
            password: "password".into(),
            month: 6,
        }
    }

    fn errors_of(value: &Sample) -> Vec<FieldError> {
        match validate(value) {
            Err(AppError::UnprocessableEntity { errors, .. }) => errors,
            Err(_) => panic!("Expected UnprocessableEntity"),
            Ok(()) => Vec::new(),
        }
    }

    #[test]
    fn test_readable_field_name() {
        assert_eq!(readable_field_name("user_id"), "User id");
        assert_eq!(readable_field_name("first_name"), "First name");
        assert_eq!(readable_field_name("email"), "Email");
        assert_eq!(readable_field_name(""), "");
    }

    #[test]
    fn test_to_field_error_required() {
        let field_err = to_field_error("title", "required");
        assert_eq!(field_err.field, "title");
        assert_eq!(field_err.message, "Title is required");
        assert_eq!(field_err.tag, Some("required".into()));
    }

    #[test]
    fn test_to_field_error_custom_tag() {
        let field_err = to_field_error("reply_to_id", "uuid");
        assert_eq!(
            field_err.message,
            "Reply to id failed validation for tag uuid"
        );
        assert_eq!(field_err.tag, Some("uuid".into()));
    }

    #[test]
    fn test_valid_passes() {
        assert!(validate(&sample()).is_ok());
    }

    #[test]
    fn test_email_error() {
        let errors = errors_of(&Sample {
            email: "not-an-email".into(),
            ..sample()
        });
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].field, "email");
        assert_eq!(errors[0].message, "Email must be a valid email address");
        assert_eq!(errors[0].tag, Some("email".into()));
    }

    #[test]
    fn test_length_errors() {
        let short = errors_of(&Sample {
            password: "short".into(),
            ..sample()
        });
        assert_eq!(
            short[0].message,
            "Password must be at least 8 characters long"
        );
        assert_eq!(short[0].tag, Some("min".into()));

        let long = errors_of(&Sample {
            password: "waytoolongpassword".into(),
            ..sample()
        });
        assert_eq!(long[0].message, "Password must not exceed 12 characters");
        assert_eq!(long[0].tag, Some("max".into()));

        // `chars` mode counts characters, not bytes (8 chars, 16 bytes).
        assert!(
            validate(&Sample {
                password: "éééééééé".into(),
                ..sample()
            })
            .is_ok()
        );
    }

    #[test]
    fn test_range_errors() {
        let low = errors_of(&Sample {
            month: 0,
            ..sample()
        });
        assert_eq!(low[0].message, "Month must be at least 1");
        assert_eq!(low[0].tag, Some("min".into()));

        let high = errors_of(&Sample {
            month: 13,
            ..sample()
        });
        assert_eq!(high[0].message, "Month must not exceed 12");
        assert_eq!(high[0].tag, Some("max".into()));
    }

    #[test]
    fn test_validation_error_top_level_message() {
        match validate(&Sample {
            email: "bad".into(),
            ..sample()
        }) {
            Err(AppError::UnprocessableEntity { error, errors }) => {
                assert_eq!(error, "Email must be a valid email address");
                assert_eq!(errors.len(), 1);
            }
            _ => panic!("Expected UnprocessableEntity"),
        }
    }
}
