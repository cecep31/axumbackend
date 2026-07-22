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
