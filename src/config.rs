use std::env;
use std::sync::OnceLock;
use std::time::Duration;

// ============================================================================
// Constants
// ============================================================================

const DEFAULT_PORT: u16 = 8080;
const DEFAULT_DATABASE_URL: &str = "postgresql://postgres:postgres@localhost:5432/axumbackend";
const DEFAULT_POOL_MAX_SIZE: usize = 20;
const DEFAULT_CONNECTION_TIMEOUT_SECS: u64 = 30;
const DEFAULT_JWT_SECRET: &str = "your-secret-key";
const DEFAULT_JWT_EXPIRY_HOURS: i64 = 3;
const DEFAULT_REFRESH_TOKEN_EXPIRY_DAYS: i64 = 30;
const DEFAULT_EMAIL_FROM: &str = "noreply@pilput.net";
const DEFAULT_FRONTEND_URL: &str = "http://localhost:3000";
const DEFAULT_FRONTEND_OAUTH_CALLBACK_URL: &str = "http://localhost:3000/auth/callback";
const DEFAULT_FRONTEND_RESET_PASSWORD_URL: &str = "http://localhost:3000/reset-password";
const DEFAULT_MAIN_DOMAIN: &str = "localhost";
const DEFAULT_RATE_LIMIT_MAX_REQUESTS: u32 = 0;
const DEFAULT_RATE_LIMIT_WINDOW_SECS: u64 = 60;
const DEFAULT_S3_ENDPOINT: &str = "localhost:9000";
const DEFAULT_S3_ACCESS_KEY: &str = "minioadmin";
const DEFAULT_S3_SECRET_KEY: &str = "minioadmin";
const DEFAULT_S3_BUCKET: &str = "minio-bucket";
const DEFAULT_CACHE_KEY_PREFIX: &str = "pilput";
const DEFAULT_CACHE_TTL_SECS: u64 = 60;
const DEFAULT_VALKEY_CONNECT_TIMEOUT_MS: u64 = 5000;
const DEFAULT_QUEUE_DEFAULT_NAME: &str = "default";
const DEFAULT_QUEUE_CONCURRENCY: u32 = 1;
const DEFAULT_QUEUE_MAX_RETRY: u32 = 5;
const DEFAULT_OPENROUTER_BASE_URL: &str = "https://openrouter.ai/api/v1";
const DEFAULT_OPENROUTER_DEFAULT_MODEL: &str = "openrouter/free";
const DEFAULT_OPENROUTER_HTTP_REFERER: &str = "https://pilput.net";
const DEFAULT_OPENROUTER_TITLE: &str = "pilput";
const DEFAULT_OPENROUTER_TIMEOUT_SECS: u64 = 90;
const DEFAULT_GITHUB_REDIRECT_URI: &str = "http://localhost:8080/api/auth/oauth/github/callback";
const DEFAULT_SMTP_PORT: u16 = 587;
const DEFAULT_SMTP_TIMEOUT_SECS: u64 = 10;
const DEFAULT_SMTP_TASK_TIMEOUT_SECS: u64 = 30;

// ============================================================================
// Configuration Structures
// ============================================================================

/// Application configuration loaded from environment variables
#[derive(Debug, Clone)]
pub struct Config {
    pub debug: bool,
    pub port: u16,
    pub database_url: String,
    pub db_pool: PoolConfig,
    pub jwt: JwtConfig,
    pub email: EmailConfig,
    pub rate_limit: RateLimitConfig,
    pub http: HttpConfig,
    pub frontend: FrontendConfig,
    pub s3: S3Config,
    pub cache: CacheConfig,
    pub queue: QueueConfig,
    pub openrouter: OpenRouterConfig,
    pub github: GitHubConfig,
    pub market: MarketConfig,
}

/// Database connection pool configuration
#[derive(Debug, Clone)]
pub struct PoolConfig {
    pub max_size: usize,
    pub connection_timeout: Duration,
}

/// JWT authentication configuration
#[derive(Debug, Clone)]
pub struct JwtConfig {
    pub secret: String,
    pub expiry_hours: i64,
    pub refresh_token_expiry_days: i64,
}

/// Email delivery configuration
#[derive(Debug, Clone)]
pub struct EmailConfig {
    pub resend_api_key: String,
    pub from: String,
    pub frontend_reset_password_url: String,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_username: String,
    pub smtp_password: String,
    pub smtp_use_tls: bool,
    pub smtp_timeout: Duration,
    pub smtp_task_timeout: Duration,
}

/// In-memory per-client, per-path rate limit configuration.
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    pub max_requests: u32,
    pub window: Duration,
}

#[derive(Debug, Clone)]
pub struct HttpConfig {
    pub trust_proxy: bool,
    pub allow_origins: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct FrontendConfig {
    pub url: String,
    pub oauth_callback_url: String,
    pub reset_password_url: String,
    pub main_domain: String,
}

#[derive(Debug, Clone)]
pub struct S3Config {
    pub endpoint: String,
    pub access_key: String,
    pub secret_key: String,
    pub bucket: String,
    pub use_ssl: bool,
}

#[derive(Debug, Clone)]
pub struct CacheConfig {
    pub valkey_url: String,
    pub key_prefix: String,
    pub ttl: Duration,
    pub connect_timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct QueueConfig {
    pub redis_url: String,
    pub connect_timeout: Duration,
    pub default_queue: String,
    pub concurrency: u32,
    pub max_retry: u32,
}

#[derive(Debug, Clone)]
pub struct OpenRouterConfig {
    pub api_key: String,
    pub base_url: String,
    pub default_model: String,
    pub http_referer: String,
    pub title: String,
    pub timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct GitHubConfig {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

/// External financial market data provider configuration.
#[derive(Debug, Clone)]
pub struct MarketConfig {
    /// RapidAPI key for the Indonesia Stock Exchange (IDX) corporate-actions API.
    /// Leave empty to disable IDX corporate-action fetching (returns empty results).
    pub rapidapi_idx_key: String,
}

static JWT_CONFIG: OnceLock<JwtConfig> = OnceLock::new();
static EMAIL_CONFIG: OnceLock<EmailConfig> = OnceLock::new();
static FRONTEND_CONFIG: OnceLock<FrontendConfig> = OnceLock::new();
static GITHUB_CONFIG: OnceLock<GitHubConfig> = OnceLock::new();
static MARKET_CONFIG: OnceLock<MarketConfig> = OnceLock::new();
static OPENROUTER_CONFIG: OnceLock<OpenRouterConfig> = OnceLock::new();
static HTTP_CONFIG: OnceLock<HttpConfig> = OnceLock::new();

impl JwtConfig {
    fn from_env() -> Self {
        Self {
            secret: env::var("JWT_SECRET").unwrap_or_else(|_| DEFAULT_JWT_SECRET.to_string()),
            expiry_hours: parse_i64("JWT_EXPIRY_HOURS", DEFAULT_JWT_EXPIRY_HOURS),
            refresh_token_expiry_days: parse_i64(
                "REFRESH_TOKEN_EXPIRY_DAYS",
                DEFAULT_REFRESH_TOKEN_EXPIRY_DAYS,
            ),
        }
    }

    pub fn init(cfg: JwtConfig) {
        JWT_CONFIG.set(cfg).expect("JwtConfig already initialized");
    }

    pub fn get() -> &'static JwtConfig {
        JWT_CONFIG.get().expect("JwtConfig not initialized")
    }
}

impl EmailConfig {
    fn from_env() -> Self {
        Self {
            resend_api_key: env::var("RESEND_API_KEY").unwrap_or_default(),
            from: env_string_alias(&["SMTP_FROM", "EMAIL_FROM"], DEFAULT_EMAIL_FROM),
            frontend_reset_password_url: env::var("FRONTEND_RESET_PASSWORD_URL")
                .unwrap_or_else(|_| DEFAULT_FRONTEND_RESET_PASSWORD_URL.to_string()),
            smtp_host: env::var("SMTP_HOST").unwrap_or_default(),
            smtp_port: parse_u16("SMTP_PORT", DEFAULT_SMTP_PORT),
            smtp_username: env::var("SMTP_USERNAME").unwrap_or_default(),
            smtp_password: env::var("SMTP_PASSWORD").unwrap_or_default(),
            smtp_use_tls: parse_bool("SMTP_TLS", false),
            smtp_timeout: Duration::from_secs(parse_u64(
                "SMTP_TIMEOUT_SECONDS",
                DEFAULT_SMTP_TIMEOUT_SECS,
            )),
            smtp_task_timeout: Duration::from_secs(parse_u64(
                "SMTP_TASK_TIMEOUT_SECONDS",
                DEFAULT_SMTP_TASK_TIMEOUT_SECS,
            )),
        }
    }

    pub fn init(cfg: EmailConfig) {
        EMAIL_CONFIG
            .set(cfg)
            .expect("EmailConfig already initialized");
    }

    pub fn get() -> &'static EmailConfig {
        EMAIL_CONFIG.get().expect("EmailConfig not initialized")
    }
}

// ============================================================================
// Implementation
// ============================================================================

impl Config {
    /// Load configuration from environment variables with sensible defaults
    ///
    /// # Environment Variables
    /// - `PORT`: Server port (default: 8080)
    /// - `DATABASE_URL`: PostgreSQL connection string
    /// - `DB_POOL_MAX_SIZE`: Maximum pool size (default: 20)
    /// - `DB_POOL_CONNECTION_TIMEOUT`: Connection timeout in seconds (default: 30)
    /// - `DB_POOL_MAX_LIFETIME`: Max connection lifetime in seconds, 0 = no limit (default: 1800)
    /// - `DB_POOL_IDLE_TIMEOUT`: Idle timeout in seconds, 0 = no limit (default: 600)
    /// - `JWT_SECRET`: Secret key for signing JWT tokens (default: "your-secret-key")
    /// - `JWT_EXPIRY_HOURS`: Access token expiry in hours (default: 3)
    /// - `RATE_LIMIT_MAX_REQUESTS`: Maximum requests per client/path window, 0 disables rate limiting (default: 0)
    /// - `RATE_LIMIT_WINDOW_SECS`: Rate limit window in seconds (default: 60)
    ///
    /// # Panics
    /// Panics if numeric values cannot be parsed.
    pub fn from_env() -> Self {
        Self {
            debug: parse_bool("APP_DEBUG", parse_bool("DEBUG", false)),
            port: parse_u16("PORT", DEFAULT_PORT),
            database_url: env::var("DATABASE_URL")
                .unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string()),
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
    fn from_env() -> Self {
        Self {
            max_size: parse_usize("DB_POOL_MAX_SIZE", DEFAULT_POOL_MAX_SIZE),
            connection_timeout: Duration::from_secs(parse_u64(
                "DB_POOL_CONNECTION_TIMEOUT",
                DEFAULT_CONNECTION_TIMEOUT_SECS,
            )),
        }
    }
}

impl RateLimitConfig {
    fn from_env() -> Self {
        Self {
            max_requests: parse_u32_alias(
                &[
                    "RATE_LIMIT_MAX_REQUESTS",
                    "HTTP_RATE_LIMIT_RPS",
                    "RATE_LIMITER_MAX",
                ],
                DEFAULT_RATE_LIMIT_MAX_REQUESTS,
            ),
            window: Duration::from_secs(parse_u64_alias(
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
    fn from_env() -> Self {
        Self {
            trust_proxy: parse_bool_alias(&["HTTP_TRUST_PROXY", "TRUST_PROXY"], false),
            allow_origins: parse_origins(
                &env::var("HTTP_ALLOW_ORIGINS").unwrap_or_else(|_| "*".to_string()),
            ),
        }
    }

    pub fn init(cfg: HttpConfig) {
        HTTP_CONFIG
            .set(cfg)
            .expect("HttpConfig already initialized");
    }

    pub fn get() -> &'static HttpConfig {
        HTTP_CONFIG.get().expect("HttpConfig not initialized")
    }
}

impl FrontendConfig {
    fn from_env() -> Self {
        Self {
            url: env::var("FRONTEND_URL").unwrap_or_else(|_| DEFAULT_FRONTEND_URL.to_string()),
            oauth_callback_url: env::var("FRONTEND_OAUTH_CALLBACK_URL")
                .unwrap_or_else(|_| DEFAULT_FRONTEND_OAUTH_CALLBACK_URL.to_string()),
            reset_password_url: env::var("FRONTEND_RESET_PASSWORD_URL")
                .unwrap_or_else(|_| DEFAULT_FRONTEND_RESET_PASSWORD_URL.to_string()),
            main_domain: env::var("MAIN_DOMAIN")
                .unwrap_or_else(|_| DEFAULT_MAIN_DOMAIN.to_string()),
        }
    }

    pub fn init(cfg: FrontendConfig) {
        FRONTEND_CONFIG
            .set(cfg)
            .expect("FrontendConfig already initialized");
    }

    pub fn get() -> &'static FrontendConfig {
        FRONTEND_CONFIG
            .get()
            .expect("FrontendConfig not initialized")
    }
}

impl S3Config {
    fn from_env() -> Self {
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
    fn from_env() -> Self {
        Self {
            valkey_url: env::var("VALKEY_URL").unwrap_or_default(),
            key_prefix: env::var("CACHE_KEY_PREFIX")
                .unwrap_or_else(|_| DEFAULT_CACHE_KEY_PREFIX.to_string()),
            ttl: Duration::from_secs(parse_u64("CACHE_TTL_SECONDS", DEFAULT_CACHE_TTL_SECS)),
            connect_timeout: Duration::from_millis(parse_u64(
                "VALKEY_CONNECT_TIMEOUT_MS",
                DEFAULT_VALKEY_CONNECT_TIMEOUT_MS,
            )),
        }
    }
}

impl QueueConfig {
    fn from_env() -> Self {
        let valkey_url = env::var("VALKEY_URL").unwrap_or_default();
        Self {
            redis_url: env_string_alias(&["QUEUE_REDIS_URL", "ASYNQ_REDIS_URL"], &valkey_url),
            connect_timeout: Duration::from_millis(parse_u64_alias(
                &[
                    "QUEUE_REDIS_TIMEOUT_MS",
                    "ASYNQ_REDIS_TIMEOUT_MS",
                    "VALKEY_CONNECT_TIMEOUT_MS",
                ],
                DEFAULT_VALKEY_CONNECT_TIMEOUT_MS,
            )),
            default_queue: env::var("QUEUE_DEFAULT_NAME")
                .unwrap_or_else(|_| DEFAULT_QUEUE_DEFAULT_NAME.to_string()),
            concurrency: parse_u32("QUEUE_CONCURRENCY", DEFAULT_QUEUE_CONCURRENCY),
            max_retry: parse_u32("QUEUE_MAX_RETRY", DEFAULT_QUEUE_MAX_RETRY),
        }
    }
}

impl OpenRouterConfig {
    fn from_env() -> Self {
        Self {
            api_key: env::var("OPENROUTER_API_KEY").unwrap_or_default(),
            base_url: env::var("OPENROUTER_BASE_URL")
                .unwrap_or_else(|_| DEFAULT_OPENROUTER_BASE_URL.to_string()),
            default_model: env::var("OPENROUTER_DEFAULT_MODEL")
                .unwrap_or_else(|_| DEFAULT_OPENROUTER_DEFAULT_MODEL.to_string()),
            http_referer: env::var("OPENROUTER_HTTP_REFERER")
                .unwrap_or_else(|_| DEFAULT_OPENROUTER_HTTP_REFERER.to_string()),
            title: env::var("OPENROUTER_TITLE")
                .unwrap_or_else(|_| DEFAULT_OPENROUTER_TITLE.to_string()),
            timeout: Duration::from_secs(parse_u64(
                "OPENROUTER_TIMEOUT_SECONDS",
                DEFAULT_OPENROUTER_TIMEOUT_SECS,
            )),
        }
    }

    pub fn init(cfg: OpenRouterConfig) {
        OPENROUTER_CONFIG
            .set(cfg)
            .expect("OpenRouterConfig already initialized");
    }

    pub fn get() -> &'static OpenRouterConfig {
        OPENROUTER_CONFIG
            .get()
            .expect("OpenRouterConfig not initialized")
    }
}

impl GitHubConfig {
    fn from_env() -> Self {
        Self {
            client_id: env::var("GITHUB_CLIENT_ID").unwrap_or_default(),
            client_secret: env::var("GITHUB_CLIENT_SECRET").unwrap_or_default(),
            redirect_uri: env::var("GITHUB_REDIRECT_URI")
                .unwrap_or_else(|_| DEFAULT_GITHUB_REDIRECT_URI.to_string()),
        }
    }

    pub fn init(cfg: GitHubConfig) {
        GITHUB_CONFIG
            .set(cfg)
            .expect("GitHubConfig already initialized");
    }

    pub fn get() -> &'static GitHubConfig {
        GITHUB_CONFIG.get().expect("GitHubConfig not initialized")
    }
}

impl MarketConfig {
    fn from_env() -> Self {
        Self {
            rapidapi_idx_key: env::var("RAPIDAPI_IDX_KEY").unwrap_or_default(),
        }
    }

    pub fn init(cfg: MarketConfig) {
        MARKET_CONFIG
            .set(cfg)
            .expect("MarketConfig already initialized");
    }

    pub fn get() -> &'static MarketConfig {
        MARKET_CONFIG.get().expect("MarketConfig not initialized")
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Parse an environment variable as u16 with default fallback.
fn parse_u16(key: &str, default: u16) -> u16 {
    env::var(key)
        .unwrap_or_else(|_| default.to_string())
        .parse::<u16>()
        .unwrap_or_else(|_| panic!("{key} must be a valid u16 number (0-65535)"))
}

/// Parse an environment variable as u64 with default fallback.
fn parse_u64(key: &str, default: u64) -> u64 {
    env::var(key)
        .unwrap_or_else(|_| default.to_string())
        .parse::<u64>()
        .unwrap_or_else(|_| panic!("{key} must be a valid u64 number"))
}

fn parse_u64_alias(keys: &[&str], default: u64) -> u64 {
    keys.iter()
        .find_map(|key| env::var(key).ok())
        .unwrap_or_else(|| default.to_string())
        .parse::<u64>()
        .unwrap_or_else(|_| panic!("{} must be a valid u64 number", keys.join(" or ")))
}

/// Parse an environment variable as u32 with default fallback.
fn parse_u32(key: &str, default: u32) -> u32 {
    env::var(key)
        .unwrap_or_else(|_| default.to_string())
        .parse::<u32>()
        .unwrap_or_else(|_| panic!("{key} must be a valid u32 number"))
}

fn parse_u32_alias(keys: &[&str], default: u32) -> u32 {
    keys.iter()
        .find_map(|key| env::var(key).ok())
        .unwrap_or_else(|| default.to_string())
        .parse::<u32>()
        .unwrap_or_else(|_| panic!("{} must be a valid u32 number", keys.join(" or ")))
}

/// Parse an environment variable as usize with default fallback.
fn parse_usize(key: &str, default: usize) -> usize {
    env::var(key)
        .unwrap_or_else(|_| default.to_string())
        .parse::<usize>()
        .unwrap_or_else(|_| panic!("{key} must be a valid usize number"))
}

/// Parse an environment variable as i64 with default fallback.
fn parse_i64(key: &str, default: i64) -> i64 {
    env::var(key)
        .unwrap_or_else(|_| default.to_string())
        .parse::<i64>()
        .unwrap_or_else(|_| panic!("{key} must be a valid i64 number"))
}

fn parse_bool(key: &str, default: bool) -> bool {
    match env::var(key) {
        Ok(value) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => default,
    }
}

fn parse_bool_alias(keys: &[&str], default: bool) -> bool {
    for key in keys {
        if let Ok(value) = env::var(key) {
            return matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            );
        }
    }
    default
}

fn env_string_alias(keys: &[&str], default: &str) -> String {
    keys.iter()
        .find_map(|key| env::var(key).ok())
        .unwrap_or_else(|| default.to_string())
}

fn parse_origins(raw: &str) -> Vec<String> {
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
