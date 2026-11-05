//! Validated request extractors.
//!
//! Replaces `axum_valid::Valid` so extraction/validation failures use the same
//! JSON envelope as echobackend:
//! - body/query/path parse failures -> `400` `{message: "Invalid request
//!   body" | "Invalid query parameters" | "Invalid path parameters", error:
//!   <parser detail>}`; [`VBind`] / [`PJson`] use echobackend's `c.Bind`
//!   message "Invalid request format" instead; a body over the route's
//!   `DefaultBodyLimit` -> `413` [`AppError::payload_too_large`]
//! - validation failures -> `422` with `errors: [{field, message, value?,
//!   tag}]`, mirroring echobackend's `response.FromValidateError` envelope
//!   (go-playground/validator): `field` is the Go struct field name
//!   (`first_name` -> `FirstName`, `user_id` -> `UserID`), `value` the
//!   submitted value when the request carried one. Like go-playground, only
//!   the first failing rule of each field is reported.

use crate::http::error::AppError;
use axum::{
    Json,
    body::Bytes,
    extract::{FromRequest, FromRequestParts, Path, Query, Request},
    http::{StatusCode, header, request::Parts},
};
use garde::i18n::{InvalidCreditCard, InvalidEmail, InvalidPhoneNumber, InvalidUrl, IpKind};
use garde::{I18n, Report, Validate, with_i18n};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::Display;

/// A single field validation failure, mirroring echobackend's
/// `validator.ValidationError` (`{field, message, value?, tag?}`).
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct FieldError {
    pub field: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
}

/// `Content-Type` is `application/json` or `application/*+json`.
fn has_json_content_type(headers: &axum::http::HeaderMap) -> bool {
    let Some(content_type) = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    let mime = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    mime == "application/json" || (mime.starts_with("application/") && mime.ends_with("+json"))
}

fn invalid_body(detail: String) -> AppError {
    AppError::bad_request("Invalid request body", Some(detail))
}

/// echobackend `response.BadRequest(c, "Invalid request format", err)`, the
/// answer to every failed `c.Bind`.
fn invalid_format(detail: String) -> AppError {
    AppError::bad_request("Invalid request format", Some(detail))
}

const EXPECTED_JSON: &str = "Expected request with `Content-Type: application/json`";

/// Buffers the body: a body over the `DefaultBodyLimit` is echobackend's
/// global 413 envelope, any other read failure goes through `on_error`.
async fn read_body<S: Send + Sync>(
    req: Request,
    state: &S,
    on_error: fn(String) -> AppError,
) -> Result<Bytes, AppError> {
    Bytes::from_request(req, state).await.map_err(|rejection| {
        if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
            AppError::payload_too_large()
        } else {
            on_error(rejection.body_text())
        }
    })
}

/// echobackend handlers pass their own message to `response.BadRequest(c,
/// "<message>", bindErr)` when `c.Bind` fails ("Failed to create post",
/// "Invalid request payload", ..). Take the body as `Result<VBind<T>,
/// AppError>` (or `VJson` / `VJsonOrDefault`) and map the rejection with
/// this: only a 400 bind rejection gets `message`, its parser detail stays as
/// `error`; the 413 body limit and 422 validation envelopes are kept as is.
pub fn bind_message(message: &'static str) -> impl FnOnce(AppError) -> AppError {
    move |err| match err {
        AppError::Detailed {
            status: StatusCode::BAD_REQUEST,
            error,
            ..
        } => AppError::Detailed {
            status: StatusCode::BAD_REQUEST,
            message: message.to_string(),
            error,
        },
        other => other,
    }
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
        if !has_json_content_type(req.headers()) {
            return Err(invalid_body(EXPECTED_JSON.to_string()));
        }
        let bytes = read_body(req, state, invalid_body).await?;
        let Json(value) = Json::<T>::from_bytes(&bytes)
            .map_err(|rejection| invalid_body(rejection.body_text()))?;
        validate_input(&value, || serde_json::from_slice(&bytes).ok())?;
        Ok(VJson(value))
    }
}

/// echobackend `c.Bind` + `c.Validate`: a zero-length body binds to
/// `T::default()` (echo's `Bind` returns early on `ContentLength == 0`) and is
/// then validated, so missing required fields are a 422 `required`; a
/// non-empty body must be JSON, and one that does not bind is a 400 "Invalid
/// request format" with the parser detail as `error`. Give `T`
/// `#[serde(default)]` so a missing field is a validation (not a bind) error.
pub struct VBind<T>(pub T);

impl<S, T> FromRequest<S> for VBind<T>
where
    S: Send + Sync,
    T: DeserializeOwned + Validate<Context = ()> + Default,
{
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let json_content_type = has_json_content_type(req.headers());
        let bytes = read_body(req, state, invalid_format).await?;
        if bytes.is_empty() {
            let value = T::default();
            validate_input(&value, || None)?;
            return Ok(VBind(value));
        }
        if !json_content_type {
            return Err(invalid_format(EXPECTED_JSON.to_string()));
        }
        let Json(value) = Json::<T>::from_bytes(&bytes)
            .map_err(|rejection| invalid_format(rejection.body_text()))?;
        validate_input(&value, || serde_json::from_slice(&bytes).ok())?;
        Ok(VBind(value))
    }
}

/// JSON body bound *without* validation, for echobackend handlers that call
/// `c.Bind` but never `c.Validate` (e.g. `UpdateHolding`). A body that does not
/// bind is a 400 "Invalid request format" with the bind error as `error`; an
/// empty body binds to `T::default()` like echo's `Bind`.
pub struct PJson<T>(pub T);

impl<S, T> FromRequest<S> for PJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned + Default,
{
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let json_content_type = has_json_content_type(req.headers());
        let bytes = read_body(req, state, invalid_format).await?;
        if bytes.is_empty() {
            return Ok(PJson(T::default()));
        }
        if !json_content_type {
            return Err(invalid_format(EXPECTED_JSON.to_string()));
        }
        let Json(value) = Json::<T>::from_bytes(&bytes)
            .map_err(|rejection| invalid_format(rejection.body_text()))?;
        Ok(PJson(value))
    }
}

/// [`VJson`] for an optional body, like echobackend's `c.Bind` (which returns
/// early on a zero-length body and whose handler tolerates `io.EOF`): an empty
/// or whitespace-only body is `T::default()` whatever the `Content-Type`, then
/// validated as usual. A non-empty body follows the [`VJson`] rules.
pub struct VJsonOrDefault<T>(pub T);

impl<S, T> FromRequest<S> for VJsonOrDefault<T>
where
    S: Send + Sync,
    T: DeserializeOwned + Validate<Context = ()> + Default,
{
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let json_content_type = has_json_content_type(req.headers());
        let bytes = read_body(req, state, invalid_body).await?;
        if bytes.iter().all(u8::is_ascii_whitespace) {
            let value = T::default();
            validate_input(&value, || None)?;
            return Ok(VJsonOrDefault(value));
        }
        if !json_content_type {
            return Err(invalid_body(EXPECTED_JSON.to_string()));
        }
        let Json(value) = Json::<T>::from_bytes(&bytes)
            .map_err(|rejection| invalid_body(rejection.body_text()))?;
        validate_input(&value, || serde_json::from_slice(&bytes).ok())?;
        Ok(VJsonOrDefault(value))
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
                    AppError::bad_request("Invalid query parameters", Some(rejection.body_text()))
                })?;
        validate_input(&value, || {
            Query::<HashMap<String, String>>::try_from_uri(&parts.uri)
                .ok()
                .and_then(|Query(map)| serde_json::to_value(map).ok())
        })?;
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
                    AppError::bad_request("Invalid path parameters", Some(rejection.body_text()))
                })?;
        validate_input(&value, || None)?;
        Ok(VPath(value))
    }
}

/// The raw (percent-decoded) value of the `name` path segment, like
/// echobackend's `c.Param(name)`; `None` when the route has no such segment.
/// Lets per-route id extractors report echobackend's own 400 message instead
/// of the generic "Invalid path parameters".
pub async fn raw_path_param<S: Send + Sync>(
    parts: &mut Parts,
    state: &S,
    name: &str,
) -> Option<String> {
    Path::<HashMap<String, String>>::from_request_parts(parts, state)
        .await
        .ok()
        .and_then(|Path(mut params)| params.remove(name))
}

/// The `name` path segment parsed as a canonical UUID (echobackend
/// `validator.IsValidUUID(c.Param(name))`); `None` when it is missing or not
/// canonical (upper-case, braced or unhyphenated ids are rejected too).
pub async fn canonical_uuid_param<S: Send + Sync>(
    parts: &mut Parts,
    state: &S,
    name: &str,
) -> Option<uuid::Uuid> {
    raw_path_param(parts, state, name)
        .await
        .and_then(|raw| crate::dto::validation::parse_canonical_uuid(&raw))
}

/// Validates `value`, mapping failures to the `422` envelope.
pub fn validate<T: Validate<Context = ()>>(value: &T) -> Result<(), AppError> {
    validate_with_source(value, || None)
}

/// Like [`validate`]; on failure `source` (the raw JSON body or query map) is
/// consulted to fill each error's `value`, as go-playground reports it.
pub fn validate_with_source<T: Validate<Context = ()>>(
    value: &T,
    source: impl FnOnce() -> Option<serde_json::Value>,
) -> Result<(), AppError> {
    with_i18n(TagI18n, || value.validate())
        .map_err(|report| validation_error(report, source(), &[]))
}

/// What the extractors use: [`validate_with_source`], with field errors in
/// struct declaration order like go-playground (garde itself reports fields
/// alphabetically), so the top-level `error` is the first declared field's.
pub fn validate_input<T: DeserializeOwned + Validate<Context = ()>>(
    value: &T,
    source: impl FnOnce() -> Option<serde_json::Value>,
) -> Result<(), AppError> {
    with_i18n(TagI18n, || value.validate())
        .map_err(|report| validation_error(report, source(), serde_field_order::<T>()))
}

/// Declared field names of a `#[derive(Deserialize)]` struct (serde passes
/// them, in declaration order, to `deserialize_struct`). Empty for anything
/// else (maps, flattened structs, enums).
fn serde_field_order<T: DeserializeOwned>() -> &'static [&'static str] {
    use serde::de::{self, Deserializer, Visitor};

    struct Introspect<'a>(&'a mut &'static [&'static str]);

    impl<'de> Deserializer<'de> for Introspect<'_> {
        type Error = de::value::Error;

        fn deserialize_any<V: Visitor<'de>>(self, _: V) -> Result<V::Value, Self::Error> {
            Err(de::Error::custom("introspection only"))
        }

        fn deserialize_struct<V: Visitor<'de>>(
            self,
            _name: &'static str,
            fields: &'static [&'static str],
            _visitor: V,
        ) -> Result<V::Value, Self::Error> {
            *self.0 = fields;
            Err(de::Error::custom("introspection only"))
        }

        serde::forward_to_deserialize_any! {
            bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
            bytes byte_buf option unit unit_struct newtype_struct seq tuple
            tuple_struct map enum identifier ignored_any
        }
    }

    let mut fields: &'static [&'static str] = &[];
    let _ = T::deserialize(Introspect(&mut fields));
    fields
}

/// `first_name` / `firstName` -> `firstname`, to match a garde path segment
/// (Rust field name) with a serde field name (possibly renamed).
fn normalized_field(name: &str) -> String {
    name.chars()
        .filter(|c| *c != '_')
        .flat_map(char::to_lowercase)
        .collect()
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

fn validation_error(
    report: Report,
    source: Option<serde_json::Value>,
    field_order: &[&str],
) -> AppError {
    // go-playground stops at the first failing tag of a field. garde runs a
    // field's custom rules first, then its built-in rules in a fixed order
    // (email/url before length before range), so keeping the first error per
    // path gives `required` before `email` before `min`/`max`.
    let mut seen = std::collections::HashSet::new();
    let order: Vec<String> = field_order.iter().map(|f| normalized_field(f)).collect();
    let mut errors: Vec<(String, &garde::Error)> = report
        .iter()
        .map(|(path, error)| (path.to_string(), error))
        .filter(|(path, _)| seen.insert(path.clone()))
        .collect();
    if !order.is_empty() {
        // Stable: errors of one field (or unknown fields) keep garde's order.
        errors.sort_by_key(|(path, _)| {
            let top = path.split(['.', '[']).next().unwrap_or_default();
            let top = normalized_field(top);
            order.iter().position(|f| *f == top).unwrap_or(usize::MAX)
        });
    }
    let field_errors: Vec<FieldError> = errors
        .into_iter()
        .map(|(path, error)| {
            let mut field_error = to_field_error(&path, error.message());
            field_error.value = source
                .as_ref()
                .and_then(|source| lookup_path(source, &path))
                .filter(|value| !value.is_null())
                .cloned();
            field_error
        })
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

/// Finds the submitted value for a garde path (`title`, `items[0].name`).
fn lookup_path<'a>(source: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    let mut current = source;
    for segment in path.split('.') {
        let (key, indexes) = segment.split_once('[').unwrap_or((segment, ""));
        if !key.is_empty() {
            current = current.get(key)?;
        }
        for index in indexes.split('[') {
            let index = index.trim_end_matches(']');
            if index.is_empty() {
                continue;
            }
            current = match index.parse::<usize>() {
                Ok(i) => current.get(i)?,
                Err(_) => current.get(index)?,
            };
        }
    }
    Some(current)
}

/// Go initialisms golint expects, as used by echobackend's DTO field names
/// (`AvatarURL`, `FolderID`, `ReplyToID`).
const GO_INITIALISMS: [&str; 4] = ["id", "url", "ip", "uuid"];

/// Go struct field name for the leaf of a garde path, i.e. what
/// go-playground's `FieldError.Field()` reports: `first_name` ->
/// `FirstName`, `user_id` -> `UserID`, `items[0]` -> `Items[0]`.
fn go_field_name(path: &str) -> String {
    let leaf = path.rsplit('.').next().unwrap_or(path);
    let (name, suffix) = match leaf.find('[') {
        Some(i) => leaf.split_at(i),
        None => (leaf, ""),
    };
    let mut out = String::with_capacity(leaf.len());
    for word in name.split('_').filter(|w| !w.is_empty()) {
        if GO_INITIALISMS.contains(&word.to_ascii_lowercase().as_str()) {
            out.push_str(&word.to_ascii_uppercase());
        } else {
            let mut chars = word.chars();
            if let Some(first) = chars.next() {
                out.extend(first.to_uppercase());
                out.push_str(chars.as_str());
            }
        }
    }
    out.push_str(suffix);
    out
}

fn to_field_error(field: &str, token: &str) -> FieldError {
    let field = go_field_name(field);
    let name = readable_field_name(&field);
    let (code, param) = token.split_once(':').unwrap_or((token, ""));
    // echobackend's validator renders `min`/`max` the same way for strings
    // and numbers ("... characters long"), so ranges reuse that wording.
    let (message, tag) = match code {
        "required" => (format!("{name} is required"), "required"),
        "email" => (format!("{name} must be a valid email address"), "email"),
        "len_min" | "min" => (
            format!("{name} must be at least {param} characters long"),
            "min",
        ),
        "len_max" | "max" => (format!("{name} must not exceed {param} characters"), "max"),
        "oneof" => (format!("{name} must be one of [{param}]"), "oneof"),
        "free_model" => (
            format!("{name} must be a free OpenRouter model (use :free suffix or openrouter/free)"),
            "free_model",
        ),
        code => (format!("{name} failed validation for tag {code}"), code),
    };
    FieldError {
        field,
        message,
        value: None,
        tag: Some(tag.to_string()),
    }
}

/// echobackend's `toReadableFieldName` over a Go field name: a space before
/// every upper-case letter after the first, everything but the first letter
/// lower-cased (`FirstName` -> `First name`, `UserID` -> `User i d`).
fn readable_field_name(field: &str) -> String {
    let mut out = String::with_capacity(field.len() + 4);
    for (i, c) in field.chars().enumerate() {
        if i == 0 {
            out.extend(c.to_uppercase());
            continue;
        }
        if c.is_uppercase() {
            out.push(' ');
        }
        out.extend(c.to_lowercase());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oneof_renders_like_go_playground() {
        let error = to_field_error("reason", "oneof:spam harassment other");
        assert_eq!(error.field, "Reason");
        assert_eq!(
            error.message,
            "Reason must be one of [spam harassment other]"
        );
        assert_eq!(error.tag.as_deref(), Some("oneof"));
    }

    #[derive(Debug, Default, serde::Deserialize, garde::Validate)]
    struct OptionalBody {
        #[garde(length(chars, max = 3))]
        name: Option<String>,
    }

    async fn optional_body(
        content_type: Option<&str>,
        body: &'static str,
    ) -> Result<OptionalBody, AppError> {
        let mut builder = axum::http::Request::builder().method("POST").uri("/");
        if let Some(content_type) = content_type {
            builder = builder.header(header::CONTENT_TYPE, content_type);
        }
        let req = builder.body(axum::body::Body::from(body)).unwrap();
        VJsonOrDefault::<OptionalBody>::from_request(req, &())
            .await
            .map(|VJsonOrDefault(value)| value)
    }

    #[tokio::test]
    async fn optional_json_body_defaults_when_empty() {
        assert!(optional_body(None, "").await.unwrap().name.is_none());
        assert!(optional_body(None, " \n").await.unwrap().name.is_none());
        assert!(
            optional_body(Some("application/json"), "")
                .await
                .unwrap()
                .name
                .is_none()
        );
        assert_eq!(
            optional_body(Some("application/json"), r#"{"name":"abc"}"#)
                .await
                .unwrap()
                .name
                .as_deref(),
            Some("abc")
        );
    }

    #[tokio::test]
    async fn optional_json_body_still_rejects_bad_bodies() {
        assert!(matches!(
            optional_body(None, r#"{"name":"abc"}"#).await,
            Err(AppError::Detailed {
                status: StatusCode::BAD_REQUEST,
                ..
            })
        ));
        assert!(matches!(
            optional_body(Some("application/json"), "{").await,
            Err(AppError::Detailed {
                status: StatusCode::BAD_REQUEST,
                ..
            })
        ));
        assert!(matches!(
            optional_body(Some("application/json"), r#"{"name":"abcd"}"#).await,
            Err(AppError::UnprocessableEntity { .. })
        ));
    }

    #[derive(Debug, Default, serde::Deserialize, garde::Validate)]
    #[serde(default)]
    struct BindBody {
        #[garde(custom(crate::dto::validation::required), length(chars, min = 3))]
        name: String,
    }

    async fn bind_body(
        content_type: Option<&str>,
        body: &'static str,
    ) -> Result<BindBody, (StatusCode, serde_json::Value)> {
        let mut builder = axum::http::Request::builder().method("POST").uri("/");
        if let Some(content_type) = content_type {
            builder = builder.header(header::CONTENT_TYPE, content_type);
        }
        let req = builder.body(axum::body::Body::from(body)).unwrap();
        match VBind::<BindBody>::from_request(req, &()).await {
            Ok(VBind(value)) => Ok(value),
            Err(err) => {
                let response = axum::response::IntoResponse::into_response(err);
                let status = response.status();
                let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .unwrap();
                Err((status, serde_json::from_slice(&bytes).unwrap()))
            }
        }
    }

    #[tokio::test]
    async fn bind_uses_invalid_request_format_for_bad_bodies() {
        for (content_type, body) in [
            (Some("application/json"), "{"),
            (Some("application/json"), r#"{"name": 5}"#),
            (Some("text/plain"), r#"{"name":"abc"}"#),
            (None, r#"{"name":"abc"}"#),
        ] {
            let (status, body) = bind_body(content_type, body).await.unwrap_err();
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert_eq!(body["message"], "Invalid request format");
            assert!(body["error"].is_string());
        }
    }

    #[tokio::test]
    async fn bind_message_rewrites_only_bind_rejections() {
        let render = |err: AppError| async move {
            let response = axum::response::IntoResponse::into_response(err);
            let status = response.status();
            let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            (
                status,
                serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
            )
        };
        let (status, body) = render(bind_message("Invalid request payload")(invalid_format(
            "EOF".into(),
        )))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            body,
            serde_json::json!({"success": false, "message": "Invalid request payload", "error": "EOF"})
        );
        let (status, body) = render(bind_message("Failed to create post")(
            AppError::payload_too_large(),
        ))
        .await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(body["message"], "Request Entity Too Large");
        let missing = BindBody::default();
        let err = validate_input(&missing, || None).unwrap_err();
        let (status, body) = render(bind_message("Failed to create post")(err)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["message"], "Validation failed");
    }

    #[tokio::test]
    async fn bind_validates_missing_and_empty_bodies_as_required() {
        for (content_type, body) in [
            (None, ""),
            (Some("application/json"), ""),
            (Some("application/json"), "{}"),
            (Some("application/json"), r#"{"name":""}"#),
        ] {
            let (status, body) = bind_body(content_type, body).await.unwrap_err();
            assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
            assert_eq!(body["message"], "Validation failed");
            assert_eq!(body["error"], "Name is required");
            assert_eq!(
                body["errors"].as_array().unwrap().len(),
                1,
                "only the first failing rule per field"
            );
            assert_eq!(body["errors"][0]["tag"], "required");
        }
        let (_, body) = bind_body(Some("application/json"), r#"{"name":"ab"}"#)
            .await
            .unwrap_err();
        assert_eq!(body["errors"][0]["tag"], "min");
        assert_eq!(
            bind_body(Some("application/json"), r#"{"name":"abc"}"#)
                .await
                .unwrap()
                .name,
            "abc"
        );
    }

    #[tokio::test]
    async fn global_body_limit_is_the_413_envelope() {
        use axum::{Router, routing::post};
        use tower::ServiceExt;
        let app = Router::new()
            .route(
                "/",
                post(
                    |VJson(body): VJson<OptionalBody>| async move { body.name.unwrap_or_default() },
                ),
            )
            .layer(axum::extract::DefaultBodyLimit::max(8));
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(axum::body::Body::from(r#"{"name":"abc"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            body,
            serde_json::json!({"success": false, "message": "Request Entity Too Large", "error": "Request body too large"})
        );
    }

    #[derive(serde::Deserialize, garde::Validate)]
    #[serde(rename_all = "camelCase")]
    struct Ordered {
        #[garde(length(min = 1))]
        zeta_field: String,
        #[garde(length(min = 1))]
        alpha: String,
    }

    #[test]
    fn field_errors_follow_declaration_order() {
        assert_eq!(serde_field_order::<Ordered>(), ["zetaField", "alpha"]);
        let value = Ordered {
            zeta_field: String::new(),
            alpha: String::new(),
        };
        match validate_input(&value, || None) {
            Err(AppError::UnprocessableEntity { error, errors }) => {
                assert_eq!(error, "Zeta field must be at least 1 characters long");
                let fields: Vec<_> = errors.iter().map(|e| e.field.as_str()).collect();
                assert_eq!(fields, ["ZetaField", "Alpha"]);
            }
            _ => panic!("Expected UnprocessableEntity"),
        }
        // garde alone reports alphabetically.
        match validate(&value) {
            Err(AppError::UnprocessableEntity { errors, .. }) => {
                assert_eq!(errors[0].field, "Alpha");
            }
            _ => panic!("Expected UnprocessableEntity"),
        }
    }

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
        assert_eq!(readable_field_name("FirstName"), "First name");
        assert_eq!(readable_field_name("Email"), "Email");
        assert_eq!(readable_field_name("UserID"), "User i d");
        assert_eq!(readable_field_name(""), "");
    }

    #[test]
    fn test_go_field_name() {
        assert_eq!(go_field_name("first_name"), "FirstName");
        assert_eq!(go_field_name("email"), "Email");
        assert_eq!(go_field_name("user_id"), "UserID");
        assert_eq!(go_field_name("avatar_url"), "AvatarURL");
        assert_eq!(go_field_name("holding_type_id"), "HoldingTypeID");
        assert_eq!(go_field_name("items[0].unit_price"), "UnitPrice");
        assert_eq!(go_field_name("tags[2]"), "Tags[2]");
    }

    #[test]
    fn test_lookup_path() {
        let source = serde_json::json!({"title": "", "items": [{"name": "x"}]});
        assert_eq!(lookup_path(&source, "title"), Some(&serde_json::json!("")));
        assert_eq!(
            lookup_path(&source, "items[0].name"),
            Some(&serde_json::json!("x"))
        );
        assert_eq!(lookup_path(&source, "missing"), None);
    }

    #[test]
    fn test_validation_error_carries_value() {
        let err = validate_with_source(
            &Sample {
                email: "not-an-email".into(),
                ..sample()
            },
            || Some(serde_json::json!({"email": "not-an-email"})),
        );
        match err {
            Err(AppError::UnprocessableEntity { errors, .. }) => {
                assert_eq!(errors[0].field, "Email");
                assert_eq!(errors[0].value, Some(serde_json::json!("not-an-email")));
                let json = serde_json::to_value(&errors[0]).unwrap();
                assert_eq!(
                    json,
                    serde_json::json!({
                        "field": "Email",
                        "message": "Email must be a valid email address",
                        "value": "not-an-email",
                        "tag": "email",
                    })
                );
            }
            _ => panic!("Expected UnprocessableEntity"),
        }
    }

    #[test]
    fn test_to_field_error_required() {
        let field_err = to_field_error("title", "required");
        assert_eq!(field_err.field, "Title");
        assert_eq!(field_err.message, "Title is required");
        assert_eq!(field_err.tag, Some("required".into()));
    }

    #[test]
    fn test_to_field_error_custom_tag() {
        let field_err = to_field_error("reply_to_id", "uuid");
        assert_eq!(field_err.field, "ReplyToID");
        assert_eq!(
            field_err.message,
            "Reply to i d failed validation for tag uuid"
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
        assert_eq!(errors[0].field, "Email");
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
        assert_eq!(low[0].message, "Month must be at least 1 characters long");
        assert_eq!(low[0].tag, Some("min".into()));

        let high = errors_of(&Sample {
            month: 13,
            ..sample()
        });
        assert_eq!(high[0].message, "Month must not exceed 12 characters");
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
