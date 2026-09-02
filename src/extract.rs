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
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use validator::Validate;

/// A single field validation failure, mirroring echobackend's
/// `validator.ValidationError` (`{field, message, value?, tag?}`).
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct FieldError {
    pub field: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
}

/// JSON body extractor with `validator` support (replaces `Valid<Json<T>>`).
pub struct VJson<T>(pub T);

impl<S, T> FromRequest<S> for VJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned + Validate,
{
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let Json(value) = Json::<T>::from_request(req, state)
            .await
            .map_err(|rejection| {
                AppError::BadRequest(format!("Invalid request body: {}", rejection.body_text()))
            })?;
        value.validate().map_err(validation_error)?;
        Ok(VJson(value))
    }
}

/// Query extractor with `validator` support (replaces `Valid<Query<T>>`).
pub struct VQuery<T>(pub T);

impl<S, T> FromRequestParts<S> for VQuery<T>
where
    S: Send + Sync,
    T: DeserializeOwned + Validate,
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
        value.validate().map_err(validation_error)?;
        Ok(VQuery(value))
    }
}

/// Path extractor with `validator` support (replaces `Valid<Path<T>>`).
pub struct VPath<T>(pub T);

impl<S, T> FromRequestParts<S> for VPath<T>
where
    S: Send + Sync,
    T: DeserializeOwned + Validate + Send,
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
        value.validate().map_err(validation_error)?;
        Ok(VPath(value))
    }
}

fn validation_error(errors: validator::ValidationErrors) -> AppError {
    let mut field_errors = Vec::new();
    for (field, errors) in errors.field_errors() {
        for error in errors {
            field_errors.push(to_field_error(field.as_ref(), error));
        }
    }
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

fn to_field_error(field: &str, error: &validator::ValidationError) -> FieldError {
    let name = readable_field_name(field);
    let (message, tag) = match error.code.as_ref() {
        "required" => (format!("{name} is required"), "required".to_string()),
        "email" => (
            format!("{name} must be a valid email address"),
            "email".to_string(),
        ),
        "length" => length_message(&name, error),
        "range" => range_message(&name, error),
        "free_model" => (
            format!("{name} must be a free OpenRouter model (use :free suffix or openrouter/free)"),
            "free_model".to_string(),
        ),
        code => (
            format!("{name} failed validation for tag {code}"),
            code.to_string(),
        ),
    };
    FieldError {
        field: field.to_string(),
        message,
        tag: Some(tag),
    }
}

fn param_u64(error: &validator::ValidationError, key: &str) -> Option<u64> {
    error.params.get(key).and_then(|value| value.as_u64())
}

fn value_len(error: &validator::ValidationError) -> Option<u64> {
    error
        .params
        .get("value")
        .and_then(|value| value.as_str())
        .map(|value| value.chars().count() as u64)
}

fn length_message(name: &str, error: &validator::ValidationError) -> (String, String) {
    let min = param_u64(error, "min");
    let max = param_u64(error, "max");
    match (min, max, value_len(error)) {
        (Some(min), _, Some(len)) if len < min => (
            format!("{name} must be at least {min} characters long"),
            "min".to_string(),
        ),
        (_, Some(max), Some(len)) if len > max => (
            format!("{name} must not exceed {max} characters"),
            "max".to_string(),
        ),
        (Some(min), _, _) => (
            format!("{name} must be at least {min} characters long"),
            "min".to_string(),
        ),
        (None, Some(max), _) => (
            format!("{name} must not exceed {max} characters"),
            "max".to_string(),
        ),
        _ => (
            format!("{name} failed validation for tag length"),
            "length".to_string(),
        ),
    }
}

fn range_message(name: &str, error: &validator::ValidationError) -> (String, String) {
    let min = param_u64(error, "min");
    let max = param_u64(error, "max");
    match (min, max) {
        (Some(min), _) => (format!("{name} must be at least {min}"), "min".to_string()),
        (None, Some(max)) => (format!("{name} must not exceed {max}"), "max".to_string()),
        _ => (
            format!("{name} failed validation for tag range"),
            "range".to_string(),
        ),
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
    use std::borrow::Cow;
    use validator::{ValidationError, ValidationErrors};

    #[test]
    fn test_readable_field_name() {
        assert_eq!(readable_field_name("user_id"), "User id");
        assert_eq!(readable_field_name("first_name"), "First name");
        assert_eq!(readable_field_name("email"), "Email");
        assert_eq!(readable_field_name(""), "");
    }

    #[test]
    fn test_to_field_error_required() {
        let err = ValidationError::new("required");
        let field_err = to_field_error("title", &err);
        assert_eq!(field_err.field, "title");
        assert_eq!(field_err.message, "Title is required");
        assert_eq!(field_err.tag, Some("required".into()));
    }

    #[test]
    fn test_to_field_error_email() {
        let err = ValidationError::new("email");
        let field_err = to_field_error("email", &err);
        assert_eq!(field_err.field, "email");
        assert_eq!(field_err.message, "Email must be a valid email address");
        assert_eq!(field_err.tag, Some("email".into()));
    }

    #[test]
    fn test_to_field_error_length() {
        let mut err = ValidationError::new("length");
        err.add_param(Cow::Borrowed("min"), &8u64);
        err.add_param(Cow::Borrowed("value"), &"short");
        let field_err = to_field_error("password", &err);
        assert_eq!(
            field_err.message,
            "Password must be at least 8 characters long"
        );
        assert_eq!(field_err.tag, Some("min".into()));

        let mut err2 = ValidationError::new("length");
        err2.add_param(Cow::Borrowed("max"), &5u64);
        err2.add_param(Cow::Borrowed("value"), &"toolongstring");
        let field_err2 = to_field_error("username", &err2);
        assert_eq!(field_err2.message, "Username must not exceed 5 characters");
        assert_eq!(field_err2.tag, Some("max".into()));
    }

    #[test]
    fn test_to_field_error_range() {
        let mut err = ValidationError::new("range");
        err.add_param(Cow::Borrowed("min"), &1u64);
        let field_err = to_field_error("month", &err);
        assert_eq!(field_err.message, "Month must be at least 1");
        assert_eq!(field_err.tag, Some("min".into()));

        let mut err2 = ValidationError::new("range");
        err2.add_param(Cow::Borrowed("max"), &12u64);
        let field_err2 = to_field_error("month", &err2);
        assert_eq!(field_err2.message, "Month must not exceed 12");
        assert_eq!(field_err2.tag, Some("max".into()));
    }

    #[test]
    fn test_validation_error_pipeline() {
        let mut errors = ValidationErrors::new();
        errors.add("email", ValidationError::new("email"));
        let app_err = validation_error(errors);
        match app_err {
            AppError::UnprocessableEntity { error, errors } => {
                assert_eq!(error, "Email must be a valid email address");
                assert_eq!(errors.len(), 1);
                assert_eq!(errors[0].field, "email");
            }
            _ => panic!("Expected UnprocessableEntity"),
        }
    }
}
