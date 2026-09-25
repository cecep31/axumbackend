/// `^[a-zA-Z0-9_-]+$`
fn is_username(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// `^[a-zA-Z0-9-]+$`
fn is_slug(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// `^[a-zA-Z0-9_-]+$`
fn is_tag(value: &str) -> bool {
    is_username(value)
}

/// Charset checks keep the `regex` tag so the 422 envelope is unchanged from
/// the previous `validator` `regex(path = ...)` rules.
fn charset(valid: bool) -> garde::Result {
    if valid {
        Ok(())
    } else {
        Err(garde::Error::new("regex"))
    }
}

pub fn username_chars<T: AsRef<str>>(value: &T, _: &()) -> garde::Result {
    charset(is_username(value.as_ref()))
}

pub fn slug_chars<T: AsRef<str>>(value: &T, _: &()) -> garde::Result {
    charset(is_slug(value.as_ref()))
}

pub fn tag_chars<T: AsRef<str>>(value: &T, _: &()) -> garde::Result {
    charset(is_tag(value.as_ref()))
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

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn test_username_charset() {
        assert!(is_username("user_123"));
        assert!(is_username("test-user"));
        assert!(is_username("Alice"));
        assert!(!is_username("user with spaces"));
        assert!(!is_username("user@email"));
        assert!(!is_username(""));
    }

    #[test]
    fn test_slug_charset() {
        assert!(is_slug("my-awesome-post-2024"));
        assert!(is_slug("post1"));
        assert!(!is_slug("post_with_underscore"));
        assert!(!is_slug("post with spaces"));
    }

    #[test]
    fn test_tag_charset() {
        assert!(is_tag("rust_lang"));
        assert!(is_tag("web-dev"));
        assert!(is_tag("backend"));
        assert!(!is_tag("tag with space"));
        assert!(!is_tag("tag!"));
    }
}
