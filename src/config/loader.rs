use std::env;
use std::str::FromStr;
use std::time::Duration;

use super::types::*;

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

// ============================================================================
// Configuration Loaders (`from_env`)
// ============================================================================

impl Config {
    /// Load configuration from environment variables with sensible defaults.
    /// The primary keys are documented in `.env.example`; legacy aliases are the
    /// extra names passed to the `*_alias` helpers below. Like echobackend,
    /// an unparsable value falls back to its default instead of failing.
    pub fn from_env() -> Self {
        Self {
            debug: parse_bool_alias(&["APP_DEBUG", "DEBUG"], false),
            port: parse_env("PORT", DEFAULT_PORT),
            database_url: env_string("DATABASE_URL", DEFAULT_DATABASE_URL),
            db_pool: PoolConfig::from_env(),
            jwt: JwtConfig::from_env(),
            email: EmailConfig::from_env(),
            rate_limit: RateLimitConfig::from_env(),
            http: HttpConfig::from_env(),
            frontend: FrontendConfig::from_env(),
            s3: S3Config::from_env(),
            cache: CacheConfig::from_env(),
            openrouter: OpenRouterConfig::from_env(),
            github: GitHubConfig::from_env(),
            market: MarketConfig::from_env(),
            checks: EnvChecks {
                cache_ttl_secs: cache_ttl_secs(),
                cache_connect_timeout_ms: cache_connect_timeout_ms(),
                smtp_port: smtp_port(),
            },
        }
    }
}

fn cache_ttl_secs() -> i64 {
    parse_env("CACHE_TTL_SECONDS", DEFAULT_CACHE_TTL_SECS as i64)
}

fn cache_connect_timeout_ms() -> i64 {
    parse_env_alias(
        &["REDIS_CONNECT_TIMEOUT_MS", "VALKEY_CONNECT_TIMEOUT_MS"],
        DEFAULT_VALKEY_CONNECT_TIMEOUT_MS,
    )
}

fn smtp_port() -> i64 {
    parse_env("SMTP_PORT", DEFAULT_SMTP_PORT)
}

/// Seconds env value (echobackend `envInt(...) * time.Second`); negatives
/// become zero here and are rejected by `validate` through [`EnvChecks`].
fn non_negative_secs(raw: i64) -> Duration {
    Duration::from_secs(raw.max(0) as u64)
}

impl PoolConfig {
    pub fn from_env() -> Self {
        Self {
            max_size: parse_env_alias(
                &["DB_POOL_MAX_OPEN", "DB_POOL_MAX_SIZE", "MAX_OPEN_CONNS"],
                DEFAULT_POOL_MAX_SIZE,
            ),
            min_idle: parse_env_alias(
                &["DB_POOL_MAX_IDLE", "MAX_IDLE_CONNS", "DB_POOL_MIN_IDLE"],
                DEFAULT_POOL_MAX_IDLE,
            ),
            // axum-only key; accepts bare seconds ("30") as well.
            connection_timeout: env::var("DB_POOL_CONNECTION_TIMEOUT")
                .ok()
                .and_then(|raw| parse_duration(&raw).ok())
                .unwrap_or(Duration::from_secs(DEFAULT_CONNECTION_TIMEOUT_SECS)),
            max_lifetime: parse_duration_alias(
                &[
                    "DB_POOL_CONN_LIFETIME",
                    "CONN_MAX_LIFETIME",
                    "DB_POOL_MAX_LIFETIME",
                ],
                Duration::from_secs(DEFAULT_POOL_CONN_LIFETIME_SECS),
            ),
            idle_timeout: parse_duration_alias(
                &[
                    "DB_POOL_CONN_IDLE_TIME",
                    "CONN_MAX_IDLE_TIME",
                    "DB_POOL_IDLE_TIMEOUT",
                ],
                Duration::from_secs(DEFAULT_POOL_CONN_IDLE_TIME_SECS),
            ),
        }
    }
}

impl JwtConfig {
    pub fn from_env() -> Self {
        let expiry = resolve_jwt_expiry(Duration::from_secs(DEFAULT_JWT_EXPIRY_SECS));
        let expiry_hours = (expiry.as_secs() / 3600) as i64;
        Self {
            secret: env_string("JWT_SECRET", DEFAULT_JWT_SECRET),
            expiry,
            expiry_hours,
            refresh_token_expiry: resolve_refresh_token_expiry(Duration::from_secs(
                DEFAULT_REFRESH_TOKEN_EXPIRY_SECS,
            )),
            refresh_token_absolute_expiry: parse_duration_alias(
                &["REFRESH_TOKEN_ABSOLUTE_EXPIRY"],
                Duration::from_secs(DEFAULT_REFRESH_TOKEN_ABSOLUTE_EXPIRY_SECS),
            ),
            refresh_token_grace_period: parse_duration_alias(
                &["REFRESH_TOKEN_GRACE_PERIOD"],
                Duration::from_secs(DEFAULT_REFRESH_TOKEN_GRACE_PERIOD_SECS),
            ),
        }
    }
}

impl EmailConfig {
    pub fn from_env() -> Self {
        Self {
            resend_api_key: env::var("RESEND_API_KEY").unwrap_or_default(),
            from: env_string_alias(&["SMTP_FROM", "EMAIL_FROM"], DEFAULT_EMAIL_FROM),
            frontend_reset_password_url: env_string(
                "FRONTEND_RESET_PASSWORD_URL",
                DEFAULT_FRONTEND_RESET_PASSWORD_URL,
            ),
            smtp_host: env::var("SMTP_HOST").unwrap_or_default(),
            smtp_port: u16::try_from(smtp_port()).unwrap_or(0),
            smtp_username: env::var("SMTP_USERNAME").unwrap_or_default(),
            smtp_password: env::var("SMTP_PASSWORD").unwrap_or_default(),
            smtp_use_tls: parse_bool("SMTP_TLS", false),
            smtp_timeout: non_negative_secs(parse_env(
                "SMTP_TIMEOUT_SECONDS",
                DEFAULT_SMTP_TIMEOUT_SECS as i64,
            )),
            smtp_task_timeout: non_negative_secs(parse_env(
                "SMTP_TASK_TIMEOUT_SECONDS",
                DEFAULT_SMTP_TASK_TIMEOUT_SECS as i64,
            )),
        }
    }
}

impl RateLimitConfig {
    pub fn from_env() -> Self {
        let window_secs: i64 = parse_env_alias(
            &[
                "HTTP_RATE_LIMIT_WINDOW_SEC",
                "RATE_LIMITER_TTL",
                "RATE_LIMIT_WINDOW_SECS",
            ],
            0,
        );
        Self {
            rps: parse_env_alias(
                &[
                    "HTTP_RATE_LIMIT_RPS",
                    "RATE_LIMITER_MAX",
                    "RATE_LIMIT_MAX_REQUESTS",
                ],
                DEFAULT_RATE_LIMIT_RPS,
            ),
            // Echo only overrides the memory store's ExpiresIn when > 0.
            idle_expiry: if window_secs > 0 {
                Duration::from_secs(window_secs as u64)
            } else {
                Duration::from_secs(DEFAULT_RATE_LIMIT_IDLE_EXPIRY_SECS)
            },
        }
    }
}

impl HttpConfig {
    pub fn from_env() -> Self {
        Self {
            trust_proxy: parse_bool_alias(&["HTTP_TRUST_PROXY", "TRUST_PROXY"], false),
            allow_origins: parse_origins(&env_string("HTTP_ALLOW_ORIGINS", "*")),
            request_timeout: non_negative_secs(parse_env_alias(
                &["HTTP_REQUEST_TIMEOUT_SECS", "REQUEST_TIMEOUT_SECS"],
                DEFAULT_HTTP_REQUEST_TIMEOUT_SECS as i64,
            )),
        }
    }
}

impl FrontendConfig {
    pub fn from_env() -> Self {
        Self {
            url: env_string("FRONTEND_URL", DEFAULT_FRONTEND_URL),
            oauth_callback_url: env_string(
                "FRONTEND_OAUTH_CALLBACK_URL",
                DEFAULT_FRONTEND_OAUTH_CALLBACK_URL,
            ),
            reset_password_url: env_string(
                "FRONTEND_RESET_PASSWORD_URL",
                DEFAULT_FRONTEND_RESET_PASSWORD_URL,
            ),
            main_domain: env_string("MAIN_DOMAIN", DEFAULT_MAIN_DOMAIN),
        }
    }
}

impl S3Config {
    pub fn from_env() -> Self {
        Self {
            endpoint: env_string_alias(&["S3_ENDPOINT", "MINIO_ENDPOINT"], DEFAULT_S3_ENDPOINT),
            access_key: env_string_alias(
                &["S3_ACCESS_KEY", "MINIO_ACCESS_KEY"],
                DEFAULT_S3_ACCESS_KEY,
            ),
            secret_key: env_string_alias(
                &["S3_SECRET_KEY", "MINIO_SECRET_KEY"],
                DEFAULT_S3_SECRET_KEY,
            ),
            bucket: env_string_alias(&["S3_BUCKET", "MINIO_BUCKET"], DEFAULT_S3_BUCKET),
            use_ssl: parse_bool_alias(&["S3_USE_SSL", "MINIO_USE_SSL"], true),
        }
    }
}

impl CacheConfig {
    pub fn from_env() -> Self {
        Self {
            valkey_url: env_string_alias(&["REDIS_URL", "VALKEY_URL"], ""),
            key_prefix: env_string("CACHE_KEY_PREFIX", DEFAULT_CACHE_KEY_PREFIX),
            ttl: non_negative_secs(cache_ttl_secs()),
            connect_timeout: Duration::from_millis(cache_connect_timeout_ms().max(0) as u64),
        }
    }
}

impl OpenRouterConfig {
    pub fn from_env() -> Self {
        Self {
            api_key: env::var("OPENROUTER_API_KEY").unwrap_or_default(),
            base_url: env_string("OPENROUTER_BASE_URL", DEFAULT_OPENROUTER_BASE_URL),
            default_model: env_string("OPENROUTER_DEFAULT_MODEL", DEFAULT_OPENROUTER_DEFAULT_MODEL),
            http_referer: env_string("OPENROUTER_HTTP_REFERER", DEFAULT_OPENROUTER_HTTP_REFERER),
            title: env_string("OPENROUTER_TITLE", DEFAULT_OPENROUTER_TITLE),
            timeout: non_negative_secs(parse_env(
                "OPENROUTER_TIMEOUT_SECONDS",
                DEFAULT_OPENROUTER_TIMEOUT_SECS as i64,
            )),
        }
    }
}

impl GitHubConfig {
    pub fn from_env() -> Self {
        Self {
            client_id: env::var("GITHUB_CLIENT_ID").unwrap_or_default(),
            client_secret: env::var("GITHUB_CLIENT_SECRET").unwrap_or_default(),
            redirect_uri: env_string("GITHUB_REDIRECT_URI", DEFAULT_GITHUB_REDIRECT_URI),
        }
    }
}

impl MarketConfig {
    pub fn from_env() -> Self {
        let key = env_string_alias(
            &["RAPIDAPI_KEY", "RAPIDAPI_IDX_KEY", "RAPIDAPI_QUOTE_KEY"],
            "",
        );
        Self {
            rapidapi_key: key.clone(),
            rapidapi_idx_key: key,
        }
    }
}

impl Config {
    /// Validates cross-section invariants and required configuration fields,
    /// matching echobackend's Config.validate().
    pub fn validate(&self) -> Result<(), String> {
        if self.jwt.secret.is_empty() {
            return Err("JWT_SECRET is required".to_string());
        }
        if self.jwt.secret.len() < 32 {
            return Err("JWT_SECRET must be at least 32 characters long".to_string());
        }
        if self.jwt.expiry.is_zero() {
            return Err("JWT_EXPIRY (or legacy JWT_EXPIRY_HOURS) must be > 0".to_string());
        }
        if self.jwt.refresh_token_expiry.is_zero() {
            return Err(
                "REFRESH_TOKEN_EXPIRY (or legacy REFRESH_TOKEN_EXPIRY_DAYS) must be > 0"
                    .to_string(),
            );
        }
        if self.jwt.refresh_token_absolute_expiry.is_zero() {
            return Err("REFRESH_TOKEN_ABSOLUTE_EXPIRY must be > 0".to_string());
        }
        // An absolute cap below the sliding window would silently override it.
        if self.jwt.refresh_token_absolute_expiry < self.jwt.refresh_token_expiry {
            return Err(
                "REFRESH_TOKEN_ABSOLUTE_EXPIRY must be >= REFRESH_TOKEN_EXPIRY".to_string(),
            );
        }
        if self.database_url.is_empty() {
            return Err("DATABASE_URL is required".to_string());
        }
        if self.checks.cache_ttl_secs < 0 {
            return Err("CACHE_TTL_SECONDS must be >= 0".to_string());
        }
        if self.checks.cache_connect_timeout_ms < 0 {
            return Err("REDIS_CONNECT_TIMEOUT_MS must be >= 0".to_string());
        }
        if self.rate_limit.rps < 0 {
            return Err("HTTP_RATE_LIMIT_RPS must be >= 0".to_string());
        }
        if !(1..=65535).contains(&self.checks.smtp_port) {
            return Err("SMTP_PORT must be between 1 and 65535".to_string());
        }
        if self.email.smtp_timeout.is_zero() {
            return Err("SMTP_TIMEOUT_SECONDS must be > 0".to_string());
        }
        if self.email.smtp_task_timeout.is_zero() {
            return Err("SMTP_TASK_TIMEOUT_SECONDS must be > 0".to_string());
        }
        Ok(())
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

    fn valid_config() -> Config {
        let mut cfg = Config::from_env();
        cfg.database_url = "postgres://localhost/test".to_string();
        cfg.jwt.secret = "this-is-a-valid-jwt-secret-with-at-least-32-chars".to_string();
        cfg.jwt.expiry = Duration::from_secs(900);
        cfg.jwt.refresh_token_expiry = Duration::from_secs(72 * 3600);
        cfg.jwt.refresh_token_absolute_expiry = Duration::from_secs(720 * 3600);
        cfg.checks = EnvChecks {
            cache_ttl_secs: 60,
            cache_connect_timeout_ms: 2000,
            smtp_port: 587,
        };
        cfg.rate_limit.rps = 0;
        cfg.email.smtp_timeout = Duration::from_secs(10);
        cfg.email.smtp_task_timeout = Duration::from_secs(30);
        cfg
    }

    #[test]
    fn test_config_validate_jwt_secret_length() {
        let mut cfg = valid_config();
        assert!(cfg.validate().is_ok());

        cfg.jwt.secret = "too-short".to_string();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_config_validate_matches_echobackend_rules() {
        let mut cfg = valid_config();
        cfg.database_url.clear();
        assert_eq!(cfg.validate().unwrap_err(), "DATABASE_URL is required");

        let mut cfg = valid_config();
        cfg.jwt.refresh_token_absolute_expiry = Duration::from_secs(3600);
        assert_eq!(
            cfg.validate().unwrap_err(),
            "REFRESH_TOKEN_ABSOLUTE_EXPIRY must be >= REFRESH_TOKEN_EXPIRY"
        );

        let mut cfg = valid_config();
        cfg.jwt.refresh_token_expiry = Duration::ZERO;
        assert_eq!(
            cfg.validate().unwrap_err(),
            "REFRESH_TOKEN_EXPIRY (or legacy REFRESH_TOKEN_EXPIRY_DAYS) must be > 0"
        );

        let mut cfg = valid_config();
        cfg.checks.cache_ttl_secs = -1;
        assert_eq!(
            cfg.validate().unwrap_err(),
            "CACHE_TTL_SECONDS must be >= 0"
        );

        let mut cfg = valid_config();
        cfg.checks.cache_connect_timeout_ms = -1;
        assert_eq!(
            cfg.validate().unwrap_err(),
            "REDIS_CONNECT_TIMEOUT_MS must be >= 0"
        );

        let mut cfg = valid_config();
        cfg.rate_limit.rps = -1;
        assert_eq!(
            cfg.validate().unwrap_err(),
            "HTTP_RATE_LIMIT_RPS must be >= 0"
        );

        for port in [0, 65536, -5] {
            let mut cfg = valid_config();
            cfg.checks.smtp_port = port;
            assert_eq!(
                cfg.validate().unwrap_err(),
                "SMTP_PORT must be between 1 and 65535"
            );
        }
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
