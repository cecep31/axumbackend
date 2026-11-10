use std::env;
use std::time::Duration;

use super::env::*;
use super::types::*;

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
            delete_account_url: env_string(
                "FRONTEND_DELETE_ACCOUNT_URL",
                DEFAULT_FRONTEND_DELETE_ACCOUNT_URL,
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
}
