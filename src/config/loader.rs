use std::env;
use std::str::FromStr;
use std::time::Duration;

use super::types::*;

// ============================================================================
// Environment Parsing Helpers
// ============================================================================

/// Reads `key` and parses it as `T`, falling back to `default` when unset.
///
/// # Panics
/// Panics if the variable is set but cannot be parsed as `T`.
pub fn parse_env<T: FromStr>(key: &str, default: T) -> T {
    parse_env_alias(&[key], default)
}

/// Like [`parse_env`], but reads the first of `keys` that is set.
///
/// # Panics
/// Panics if the variable is set but cannot be parsed as `T`.
pub fn parse_env_alias<T: FromStr>(keys: &[&str], default: T) -> T {
    let Some(raw) = keys.iter().find_map(|key| env::var(key).ok()) else {
        return default;
    };
    raw.parse().unwrap_or_else(|_| {
        panic!(
            "{} must be a valid {} number",
            keys.join(" or "),
            std::any::type_name::<T>()
        )
    })
}

/// Parse a human-readable duration string into std::time::Duration.
///
/// Supports Go-style duration formats such as "15m", "1h", "30s", "500ms", "1d", "1h30m",
/// or plain numbers interpreted as seconds (e.g. "900").
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

pub fn parse_duration_alias(keys: &[&str], default: Duration) -> Duration {
    for key in keys {
        if let Ok(value) = env::var(key) {
            let val = value.trim();
            if !val.is_empty()
                && let Ok(d) = parse_duration(val)
            {
                return d;
            }
        }
    }
    default
}

pub fn resolve_jwt_expiry(default_duration: Duration) -> Duration {
    if let Ok(val) = env::var("JWT_EXPIRY")
        && let Ok(d) = parse_duration(&val)
    {
        return d;
    }
    if let Ok(val) = env::var("JWT_EXPIRY_HOURS")
        && let Ok(hours) = val.trim().parse::<u64>()
    {
        return Duration::from_secs(hours * 3600);
    }
    default_duration
}

pub fn parse_bool(key: &str, default: bool) -> bool {
    parse_bool_alias(&[key], default)
}

pub fn parse_bool_alias(keys: &[&str], default: bool) -> bool {
    keys.iter()
        .find_map(|key| env::var(key).ok())
        .map_or(default, |value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
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
    /// extra names passed to the `*_alias` helpers below.
    ///
    /// # Panics
    /// Panics if a numeric value is set but cannot be parsed.
    pub fn from_env() -> Self {
        Self {
            debug: parse_bool("APP_DEBUG", parse_bool("DEBUG", false)),
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
            queue: QueueConfig::from_env(),
            openrouter: OpenRouterConfig::from_env(),
            github: GitHubConfig::from_env(),
            market: MarketConfig::from_env(),
        }
    }
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
            connection_timeout: parse_duration_alias(
                &["DB_POOL_CONNECTION_TIMEOUT"],
                Duration::from_secs(DEFAULT_CONNECTION_TIMEOUT_SECS),
            ),
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
            refresh_token_expiry_days: parse_env(
                "REFRESH_TOKEN_EXPIRY_DAYS",
                DEFAULT_REFRESH_TOKEN_EXPIRY_DAYS,
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
            smtp_port: parse_env("SMTP_PORT", DEFAULT_SMTP_PORT),
            smtp_username: env::var("SMTP_USERNAME").unwrap_or_default(),
            smtp_password: env::var("SMTP_PASSWORD").unwrap_or_default(),
            smtp_use_tls: parse_bool("SMTP_TLS", false),
            smtp_timeout: Duration::from_secs(parse_env(
                "SMTP_TIMEOUT_SECONDS",
                DEFAULT_SMTP_TIMEOUT_SECS,
            )),
            smtp_task_timeout: Duration::from_secs(parse_env(
                "SMTP_TASK_TIMEOUT_SECONDS",
                DEFAULT_SMTP_TASK_TIMEOUT_SECS,
            )),
        }
    }
}

impl RateLimitConfig {
    pub fn from_env() -> Self {
        Self {
            max_requests: parse_env_alias(
                &[
                    "RATE_LIMIT_MAX_REQUESTS",
                    "HTTP_RATE_LIMIT_RPS",
                    "RATE_LIMITER_MAX",
                ],
                DEFAULT_RATE_LIMIT_MAX_REQUESTS,
            ),
            window: Duration::from_secs(parse_env_alias(
                &[
                    "RATE_LIMIT_WINDOW_SECS",
                    "HTTP_RATE_LIMIT_WINDOW_SEC",
                    "RATE_LIMITER_TTL",
                ],
                DEFAULT_RATE_LIMIT_WINDOW_SECS,
            )),
        }
    }
}

impl HttpConfig {
    pub fn from_env() -> Self {
        Self {
            trust_proxy: parse_bool_alias(&["HTTP_TRUST_PROXY", "TRUST_PROXY"], false),
            allow_origins: parse_origins(&env_string("HTTP_ALLOW_ORIGINS", "*")),
            request_timeout: Duration::from_secs(parse_env_alias(
                &["HTTP_REQUEST_TIMEOUT_SECS", "REQUEST_TIMEOUT_SECS"],
                DEFAULT_HTTP_REQUEST_TIMEOUT_SECS,
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
            ttl: Duration::from_secs(parse_env("CACHE_TTL_SECONDS", DEFAULT_CACHE_TTL_SECS)),
            connect_timeout: Duration::from_millis(parse_env_alias(
                &["REDIS_CONNECT_TIMEOUT_MS", "VALKEY_CONNECT_TIMEOUT_MS"],
                DEFAULT_VALKEY_CONNECT_TIMEOUT_MS,
            )),
        }
    }
}

impl QueueConfig {
    pub fn from_env() -> Self {
        let default_redis_url = env_string_alias(&["REDIS_URL", "VALKEY_URL"], "");
        Self {
            redis_url: env_string_alias(
                &[
                    "QUEUE_REDIS_URL",
                    "ASYNQ_REDIS_URL",
                    "REDIS_URL",
                    "VALKEY_URL",
                ],
                &default_redis_url,
            ),
            connect_timeout: Duration::from_millis(parse_env_alias(
                &[
                    "QUEUE_REDIS_TIMEOUT_MS",
                    "ASYNQ_REDIS_TIMEOUT_MS",
                    "REDIS_CONNECT_TIMEOUT_MS",
                    "VALKEY_CONNECT_TIMEOUT_MS",
                ],
                DEFAULT_VALKEY_CONNECT_TIMEOUT_MS,
            )),
            default_queue: env_string("QUEUE_DEFAULT_NAME", DEFAULT_QUEUE_DEFAULT_NAME),
            concurrency: parse_env("QUEUE_CONCURRENCY", DEFAULT_QUEUE_CONCURRENCY),
            max_retry: parse_env("QUEUE_MAX_RETRY", DEFAULT_QUEUE_MAX_RETRY),
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
            timeout: Duration::from_secs(parse_env(
                "OPENROUTER_TIMEOUT_SECONDS",
                DEFAULT_OPENROUTER_TIMEOUT_SECS,
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
        if self.jwt.refresh_token_expiry_days <= 0 {
            return Err("REFRESH_TOKEN_EXPIRY_DAYS must be > 0".to_string());
        }
        if self.database_url.is_empty() {
            return Err("DATABASE_URL is required".to_string());
        }
        if self.email.smtp_port == 0 {
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

    #[test]
    fn test_config_validate_jwt_secret_length() {
        let mut cfg = Config::from_env();
        cfg.jwt.secret = "too-short".to_string();
        assert!(cfg.validate().is_err());

        cfg.jwt.secret = "this-is-a-valid-jwt-secret-with-at-least-32-chars".to_string();
        assert!(cfg.validate().is_ok());
    }
}
