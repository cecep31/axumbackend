use once_cell::sync::Lazy;
use regex::Regex;

pub static USERNAME_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[a-zA-Z0-9_-]+$").unwrap());
pub static SLUG_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[a-zA-Z0-9-]+$").unwrap());
pub static TAG_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[a-zA-Z0-9_-]+$").unwrap());

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
    fn test_username_regex() {
        assert!(USERNAME_RE.is_match("user_123"));
        assert!(USERNAME_RE.is_match("test-user"));
        assert!(USERNAME_RE.is_match("Alice"));
        assert!(!USERNAME_RE.is_match("user with spaces"));
        assert!(!USERNAME_RE.is_match("user@email"));
        assert!(!USERNAME_RE.is_match(""));
    }

    #[test]
    fn test_slug_regex() {
        assert!(SLUG_RE.is_match("my-awesome-post-2024"));
        assert!(SLUG_RE.is_match("post1"));
        assert!(!SLUG_RE.is_match("post_with_underscore"));
        assert!(!SLUG_RE.is_match("post with spaces"));
    }

    #[test]
    fn test_tag_regex() {
        assert!(TAG_RE.is_match("rust_lang"));
        assert!(TAG_RE.is_match("web-dev"));
        assert!(TAG_RE.is_match("backend"));
        assert!(!TAG_RE.is_match("tag with space"));
        assert!(!TAG_RE.is_match("tag!"));
    }
}
