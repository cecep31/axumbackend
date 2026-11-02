use std::env;
use std::str::FromStr;
use std::time::Duration;

// ============================================================================
// Environment Parsing Helpers
// ============================================================================
//
// These mirror echobackend's `config/env.go`: the FIRST set key among the
// aliases wins, and when its value does not parse the default is returned
// (later aliases are not consulted). Nothing here panics.

/// The first of `keys` that is set (even to an empty string), with its value.
fn first_set<'a>(keys: &[&'a str]) -> Option<(&'a str, String)> {
    keys.iter()
        .find_map(|key| env::var(key).ok().map(|value| (*key, value)))
}

/// Reads `key` and parses it as `T`, falling back to `default` when unset or
/// unparsable (echobackend `envInt`).
pub fn parse_env<T: FromStr>(key: &str, default: T) -> T {
    parse_env_alias(&[key], default)
}

/// Like [`parse_env`], but reads the first of `keys` that is set. An
/// unparsable value logs a warning and yields `default`.
pub fn parse_env_alias<T: FromStr>(keys: &[&str], default: T) -> T {
    let Some((key, raw)) = first_set(keys) else {
        return default;
    };
    match raw.parse() {
        Ok(value) => value,
        Err(_) => {
            tracing::warn!(key, value = %raw, "config: invalid number, using default");
            default
        }
    }
}

/// Lenient duration parser for axum-only keys: Go-style strings ("15m",
/// "1h30m", "500ms", plus "1d") or plain numbers as seconds ("900").
/// Keys shared with echobackend use the strict [`parse_go_duration`].
pub fn parse_duration(raw: &str) -> Result<Duration, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("empty duration string".to_string());
    }

    if let Ok(secs) = raw.parse::<u64>() {
        return Ok(Duration::from_secs(secs));
    }

    let mut total_millis: u64 = 0;
    let mut num_buf = String::new();
    let mut chars = raw.chars().peekable();

    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() || c == '.' {
            num_buf.push(c);
            chars.next();
        } else if c.is_alphabetic() || c == 'µ' {
            let mut unit_buf = String::new();
            while let Some(&u) = chars.peek() {
                if u.is_alphabetic() || u == 'µ' {
                    unit_buf.push(u);
                    chars.next();
                } else {
                    break;
                }
            }

            if num_buf.is_empty() {
                return Err(format!(
                    "missing number before unit '{unit_buf}' in '{raw}'"
                ));
            }

            let val: f64 = num_buf
                .parse()
                .map_err(|_| format!("invalid numeric value in duration: '{num_buf}'"))?;
            num_buf.clear();

            let unit = unit_buf.to_ascii_lowercase();
            let millis = match unit.as_str() {
                "d" => val * 86_400_000.0,
                "h" => val * 3_600_000.0,
                "m" => val * 60_000.0,
                "s" => val * 1_000.0,
                "ms" => val,
                "us" | "µs" => val / 1_000.0,
                "ns" => val / 1_000_000.0,
                _ => return Err(format!("unknown duration unit '{unit}' in '{raw}'")),
            };
            total_millis = total_millis.saturating_add(millis.round() as u64);
        } else if c.is_whitespace() {
            chars.next();
        } else {
            return Err(format!("unexpected character '{c}' in duration '{raw}'"));
        }
    }

    if !num_buf.is_empty() {
        let val: u64 = num_buf
            .parse()
            .map_err(|_| format!("invalid numeric value in duration: '{num_buf}'"))?;
        total_millis = total_millis.saturating_add(val * 1000);
    }

    Ok(Duration::from_millis(total_millis))
}

/// Strict Go `time.ParseDuration`: `[-+]?(<number><unit>)+` or `"0"`, units
/// `ns us µs μs ms s m h` (case-sensitive, no spaces, no days, no bare
/// numbers). A negative value maps to `Duration::ZERO`, so the `> 0` checks
/// in [`Config::validate`] reject it as echobackend does.
pub fn parse_go_duration(raw: &str) -> Result<Duration, String> {
    let invalid = || format!("time: invalid duration \"{raw}\"");
    let (negative, mut rest) = match raw.as_bytes().first() {
        Some(b'-') => (true, &raw[1..]),
        Some(b'+') => (false, &raw[1..]),
        _ => (false, raw),
    };
    if rest == "0" {
        return Ok(Duration::ZERO);
    }
    if rest.is_empty() {
        return Err(invalid());
    }

    let mut total_nanos: f64 = 0.0;
    while !rest.is_empty() {
        let number_len = rest
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(rest.len());
        let number = &rest[..number_len];
        if number.is_empty() || number == "." || number.matches('.').count() > 1 {
            return Err(invalid());
        }
        let value: f64 = number.parse().map_err(|_| invalid())?;
        rest = &rest[number_len..];

        let unit_len = rest
            .find(|c: char| c.is_ascii_digit() || c == '.')
            .unwrap_or(rest.len());
        let nanos_per_unit = match &rest[..unit_len] {
            "ns" => 1.0,
            "us" | "\u{b5}s" | "\u{3bc}s" => 1e3,
            "ms" => 1e6,
            "s" => 1e9,
            "m" => 60e9,
            "h" => 3600e9,
            _ => return Err(invalid()),
        };
        rest = &rest[unit_len..];
        total_nanos += value * nanos_per_unit;
    }

    if negative {
        return Ok(Duration::ZERO);
    }
    Ok(Duration::from_nanos(
        total_nanos.round().min(u64::MAX as f64) as u64,
    ))
}

/// First set key among `keys`, parsed with [`parse_go_duration`]; an invalid
/// value yields `default` (echobackend `envDuration`).
pub fn parse_duration_alias(keys: &[&str], default: Duration) -> Duration {
    let Some((key, raw)) = first_set(keys) else {
        return default;
    };
    parse_go_duration(&raw).unwrap_or_else(|_| {
        tracing::warn!(key, value = %raw, "config: invalid duration, using default");
        default
    })
}

/// echobackend `resolveJWTExpiry`: JWT_EXPIRY (Go duration), else the legacy
/// JWT_EXPIRY_HOURS (whole hours), else `default_duration`.
pub fn resolve_jwt_expiry(default_duration: Duration) -> Duration {
    resolve_duration_with_legacy("JWT_EXPIRY", "JWT_EXPIRY_HOURS", 3600, default_duration)
}

/// echobackend `resolveRefreshTokenExpiry`: REFRESH_TOKEN_EXPIRY (Go
/// duration, e.g. "72h"), else the legacy REFRESH_TOKEN_EXPIRY_DAYS (whole
/// days), else `default_duration`.
pub fn resolve_refresh_token_expiry(default_duration: Duration) -> Duration {
    resolve_duration_with_legacy(
        "REFRESH_TOKEN_EXPIRY",
        "REFRESH_TOKEN_EXPIRY_DAYS",
        24 * 3600,
        default_duration,
    )
}

fn resolve_duration_with_legacy(
    key: &str,
    legacy_key: &str,
    legacy_unit_secs: u64,
    default_duration: Duration,
) -> Duration {
    if let Ok(raw) = env::var(key)
        && let Ok(duration) = parse_go_duration(&raw)
    {
        return duration;
    }
    if let Ok(raw) = env::var(legacy_key)
        && let Ok(units) = raw.parse::<i64>()
    {
        // A negative legacy value becomes zero and fails validation.
        return Duration::from_secs(units.max(0) as u64 * legacy_unit_secs);
    }
    default_duration
}

/// Go `strconv.ParseBool`.
pub fn go_parse_bool(raw: &str) -> Option<bool> {
    match raw {
        "1" | "t" | "T" | "TRUE" | "true" | "True" => Some(true),
        "0" | "f" | "F" | "FALSE" | "false" | "False" => Some(false),
        _ => None,
    }
}

pub fn parse_bool(key: &str, default: bool) -> bool {
    parse_bool_alias(&[key], default)
}

/// echobackend `envBool`: the first set key is parsed with Go
/// `strconv.ParseBool`; any other value yields `default`.
pub fn parse_bool_alias(keys: &[&str], default: bool) -> bool {
    let Some((key, raw)) = first_set(keys) else {
        return default;
    };
    go_parse_bool(&raw).unwrap_or_else(|| {
        tracing::warn!(key, value = %raw, "config: invalid boolean, using default");
        default
    })
}

pub fn env_string(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_string())
}

pub fn env_string_alias(keys: &[&str], default: &str) -> String {
    keys.iter()
        .find_map(|key| env::var(key).ok())
        .unwrap_or_else(|| default.to_string())
}

pub fn parse_origins(raw: &str) -> Vec<String> {
    let raw = raw.trim();
    if raw.is_empty() || raw == "*" {
        return vec!["*".to_string()];
    }

    let origins: Vec<String> = raw
        .split(',')
        .map(str::trim)
        .filter(|origin| !origin.is_empty())
        .map(ToOwned::to_owned)
        .collect();

    if origins.is_empty() {
        vec!["*".to_string()]
    } else {
        origins
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_origins_wildcard_and_empty() {
        assert_eq!(parse_origins("*"), vec!["*".to_string()]);
        assert_eq!(parse_origins(""), vec!["*".to_string()]);
        assert_eq!(parse_origins("   "), vec!["*".to_string()]);
        assert_eq!(parse_origins(",,"), vec!["*".to_string()]);
    }

    #[test]
    fn test_parse_origins_multiple_domains() {
        let origins =
            parse_origins("http://localhost:3000, https://example.com , https://app.example.com");
        assert_eq!(
            origins,
            vec![
                "http://localhost:3000".to_string(),
                "https://example.com".to_string(),
                "https://app.example.com".to_string()
            ]
        );
    }

    #[test]
    fn test_parse_numeric_defaults() {
        let non_existent = "TEST_NON_EXISTENT_VAR_XYZ_987";
        assert_eq!(parse_env::<u16>(non_existent, 8080), 8080);
        assert_eq!(parse_env::<u32>(non_existent, 100), 100);
        assert_eq!(parse_env::<u64>(non_existent, 5000), 5000);
        assert_eq!(parse_env::<usize>(non_existent, 20), 20);
        assert_eq!(parse_env::<i64>(non_existent, -42), -42);
    }

    #[test]
    fn test_parse_bool_fallback() {
        let non_existent = "TEST_NON_EXISTENT_VAR_XYZ_987";
        assert!(parse_bool(non_existent, true));
        assert!(!parse_bool(non_existent, false));
    }

    #[test]
    fn test_env_string_alias_fallback() {
        let res = env_string_alias(
            &["TEST_NON_EXISTENT_1", "TEST_NON_EXISTENT_2"],
            "default_val",
        );
        assert_eq!(res, "default_val");
    }

    #[test]
    fn test_parse_duration() {
        assert_eq!(parse_duration("15m").unwrap(), Duration::from_secs(15 * 60));
        assert_eq!(parse_duration("1h").unwrap(), Duration::from_secs(3600));
        assert_eq!(parse_duration("30s").unwrap(), Duration::from_secs(30));
        assert_eq!(parse_duration("500ms").unwrap(), Duration::from_millis(500));
        assert_eq!(parse_duration("1h30m").unwrap(), Duration::from_secs(5400));
        assert_eq!(parse_duration("900").unwrap(), Duration::from_secs(900));
        assert!(parse_duration("").is_err());
    }

    #[test]
    fn test_parse_go_duration() {
        assert_eq!(
            parse_go_duration("72h").unwrap(),
            Duration::from_secs(72 * 3600)
        );
        assert_eq!(
            parse_go_duration("1h30m").unwrap(),
            Duration::from_secs(5400)
        );
        assert_eq!(
            parse_go_duration("1.5s").unwrap(),
            Duration::from_millis(1500)
        );
        assert_eq!(
            parse_go_duration("500ms").unwrap(),
            Duration::from_millis(500)
        );
        assert_eq!(parse_go_duration("0").unwrap(), Duration::ZERO);
        assert_eq!(parse_go_duration("-5m").unwrap(), Duration::ZERO);
        // Go rejects bare numbers, days, spaces and upper-case units.
        for raw in ["", "900", "1d", "15 m", "15M", "h", "1.2.3s"] {
            assert!(parse_go_duration(raw).is_err(), "{raw:?} should be invalid");
        }
    }

    #[test]
    fn test_go_parse_bool() {
        for raw in ["1", "t", "T", "TRUE", "true", "True"] {
            assert_eq!(go_parse_bool(raw), Some(true));
        }
        for raw in ["0", "f", "F", "FALSE", "false", "False"] {
            assert_eq!(go_parse_bool(raw), Some(false));
        }
        for raw in ["yes", "on", " true", "tRUE", ""] {
            assert_eq!(go_parse_bool(raw), None);
        }
    }
}
