/// go-playground `required` for a string: empty is tag `required`. List it
/// first (`#[garde(custom(required), ...)]`) on a `#[serde(default)]` field:
/// custom rules run before built-in ones and only the first error per field
/// is reported, so a missing/empty value never also reports `min`/`email`.
pub fn required(value: &str, _: &()) -> garde::Result {
    if value.is_empty() {
        Err(garde::Error::new("required"))
    } else {
        Ok(())
    }
}

/// go-playground `required` for a number: the zero value is tag `required`.
/// Like [`required`], list it first on a `#[serde(default)]` field.
pub fn required_num<T: Default + PartialEq>(value: &T, _: &()) -> garde::Result {
    if *value == T::default() {
        Err(garde::Error::new("required"))
    } else {
        Ok(())
    }
}

/// echobackend `validator.IsValidUUID`: exactly 36 chars, lowercase hex,
/// hyphens at 8/13/18/23. Stricter than `Uuid::parse_str`, which also takes
/// upper-case, simple, braced and urn forms.
pub fn is_canonical_uuid(raw: &str) -> bool {
    raw.len() == 36
        && raw.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_digit() || (b'a'..=b'f').contains(&b),
        })
}

/// Parses `raw` only when it is a canonical UUID ([`is_canonical_uuid`]).
pub fn parse_canonical_uuid(raw: &str) -> Option<uuid::Uuid> {
    if is_canonical_uuid(raw) {
        uuid::Uuid::parse_str(raw).ok()
    } else {
        None
    }
}

/// Mirrors echobackend's `ParsePaginationParams(defaultLimit)`: an invalid or
/// missing value silently falls back to the default instead of failing the
/// request, and `limit` is clamped to 100 while `offset` has no upper bound.
pub fn parse_pagination(
    offset: Option<&str>,
    limit: Option<&str>,
    default_limit: i64,
) -> (i64, i64) {
    let mut limit_val = default_limit;
    if let Some(parsed) = limit.and_then(|v| v.parse::<i64>().ok())
        && parsed > 0
    {
        limit_val = parsed;
    }
    if limit_val > 100 {
        limit_val = 100;
    }

    let mut offset_val = 0i64;
    if let Some(parsed) = offset.and_then(|v| v.parse::<i64>().ok())
        && parsed >= 0
    {
        offset_val = parsed;
    }

    (limit_val, offset_val)
}

/// Mirrors echobackend's inline `month` query parsing: only a valid `1..=12`
/// integer overrides the caller's default; anything else (missing, empty,
/// non-numeric, out of range) is ignored.
pub fn parse_month(raw: Option<&str>) -> Option<i32> {
    raw.and_then(|v| v.parse::<i32>().ok())
        .filter(|v| (1..=12).contains(v))
}

/// Mirrors echobackend's inline `year` query parsing: any parseable integer
/// is accepted (no range bound); non-numeric/missing values are ignored.
pub fn parse_year(raw: Option<&str>) -> Option<i32> {
    raw.and_then(|v| v.parse::<i32>().ok())
}

/// Test helper: the `(field, tag)` pairs of the 422 envelope `value` would
/// produce through the extractors (`validate_input`), empty when it passes.
#[cfg(test)]
pub fn failing_tags<T: serde::de::DeserializeOwned + garde::Validate<Context = ()>>(
    value: &T,
) -> Vec<(String, String)> {
    match crate::http::extract::validate_input(value, || None) {
        Ok(()) => Vec::new(),
        Err(crate::http::error::AppError::UnprocessableEntity { errors, .. }) => errors
            .into_iter()
            .map(|e| (e.field, e.tag.unwrap_or_default()))
            .collect(),
        Err(other) => panic!("expected a 422, got {other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canonical_uuid() {
        let ok = "0190a8f2-1c3e-7b4a-9d6e-2f1a3b4c5d6e";
        assert!(is_canonical_uuid(ok));
        assert_eq!(parse_canonical_uuid(ok).unwrap().to_string(), ok);
        for bad in [
            "",
            "not-a-uuid",
            "0190A8F2-1C3E-7B4A-9D6E-2F1A3B4C5D6E",
            "0190a8f21c3e7b4a9d6e2f1a3b4c5d6e",
            "{0190a8f2-1c3e-7b4a-9d6e-2f1a3b4c5d6e}",
            "urn:uuid:0190a8f2-1c3e-7b4a-9d6e-2f1a3b4c5d6e",
            "0190a8f2-1c3e-7b4a-9d6e-2f1a3b4c5d6g",
        ] {
            assert!(!is_canonical_uuid(bad), "{bad}");
            assert!(parse_canonical_uuid(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn test_required() {
        assert!(required("", &()).is_err());
        assert!(required(" ", &()).is_ok());
        assert!(required_num(&0i32, &()).is_err());
        assert!(required_num(&-1i32, &()).is_ok());
    }

    #[test]
    fn test_parse_pagination_defaults() {
        let (limit, offset) = parse_pagination(None, None, 10);
        assert_eq!(limit, 10);
        assert_eq!(offset, 0);
    }

    #[test]
    fn test_parse_pagination_valid_values() {
        let (limit, offset) = parse_pagination(Some("20"), Some("50"), 10);
        assert_eq!(limit, 50);
        assert_eq!(offset, 20);
    }

    #[test]
    fn test_parse_pagination_clamp_limit() {
        let (limit, offset) = parse_pagination(Some("0"), Some("500"), 10);
        assert_eq!(limit, 100);
        assert_eq!(offset, 0);
    }

    #[test]
    fn test_parse_pagination_invalid_inputs_fallback() {
        let (limit, offset) = parse_pagination(Some("invalid"), Some("not-a-number"), 15);
        assert_eq!(limit, 15);
        assert_eq!(offset, 0);

        let (limit, offset) = parse_pagination(Some("-5"), Some("0"), 15);
        assert_eq!(limit, 15);
        assert_eq!(offset, 0);
    }

    #[test]
    fn test_parse_month_valid() {
        assert_eq!(parse_month(Some("1")), Some(1));
        assert_eq!(parse_month(Some("6")), Some(6));
        assert_eq!(parse_month(Some("12")), Some(12));
    }

    #[test]
    fn test_parse_month_invalid() {
        assert_eq!(parse_month(None), None);
        assert_eq!(parse_month(Some("")), None);
        assert_eq!(parse_month(Some("0")), None);
        assert_eq!(parse_month(Some("13")), None);
        assert_eq!(parse_month(Some("-1")), None);
        assert_eq!(parse_month(Some("abc")), None);
    }

    #[test]
    fn test_parse_year_valid() {
        assert_eq!(parse_year(Some("2024")), Some(2024));
        assert_eq!(parse_year(Some("1999")), Some(1999));
        assert_eq!(parse_year(Some("-100")), Some(-100));
    }

    #[test]
    fn test_parse_year_invalid() {
        assert_eq!(parse_year(None), None);
        assert_eq!(parse_year(Some("")), None);
        assert_eq!(parse_year(Some("year")), None);
    }
}
