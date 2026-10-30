use std::time::Duration;

// ============================================================================
// Constants
// ============================================================================

pub const DEFAULT_PORT: u16 = 8080;
/// Like echobackend, `DATABASE_URL` has no default: an empty value fails
/// `Config::validate` with "DATABASE_URL is required".
pub const DEFAULT_DATABASE_URL: &str = "";
pub const DEFAULT_POOL_MAX_SIZE: usize = 25;
pub const DEFAULT_POOL_MAX_IDLE: usize = 25;
pub const DEFAULT_POOL_CONN_LIFETIME_SECS: u64 = 15 * 60;
pub const DEFAULT_POOL_CONN_IDLE_TIME_SECS: u64 = 5 * 60;
pub const DEFAULT_CONNECTION_TIMEOUT_SECS: u64 = 30;
/// Like echobackend, no default secret: empty fails "JWT_SECRET is required".
pub const DEFAULT_JWT_SECRET: &str = "";
pub const DEFAULT_JWT_EXPIRY_SECS: u64 = 15 * 60;
pub const DEFAULT_JWT_EXPIRY_HOURS: i64 = 0;
/// Matches echobackend's REFRESH_TOKEN_EXPIRY default (72h).
pub const DEFAULT_REFRESH_TOKEN_EXPIRY_SECS: u64 = 3 * 24 * 60 * 60;
/// Matches echobackend's REFRESH_TOKEN_ABSOLUTE_EXPIRY default (720h).
pub const DEFAULT_REFRESH_TOKEN_ABSOLUTE_EXPIRY_SECS: u64 = 30 * 24 * 60 * 60;
/// Matches echobackend's REFRESH_TOKEN_GRACE_PERIOD default (60s).
pub const DEFAULT_REFRESH_TOKEN_GRACE_PERIOD_SECS: u64 = 60;
pub const DEFAULT_EMAIL_FROM: &str = "noreply@pilput.net";
pub const DEFAULT_FRONTEND_URL: &str = "http://localhost:3000";
pub const DEFAULT_FRONTEND_OAUTH_CALLBACK_URL: &str = "http://localhost:3000/auth/callback";
pub const DEFAULT_FRONTEND_RESET_PASSWORD_URL: &str = "http://localhost:3000/reset-password";
pub const DEFAULT_MAIN_DOMAIN: &str = "localhost";
/// HTTP_RATE_LIMIT_RPS default: 0 disables the global limiter (echobackend).
pub const DEFAULT_RATE_LIMIT_RPS: i64 = 0;
/// Idle visitor expiry of the global limiter when HTTP_RATE_LIMIT_WINDOW_SEC
/// is 0 (Echo's memory store default, 3 minutes).
pub const DEFAULT_RATE_LIMIT_IDLE_EXPIRY_SECS: u64 = 3 * 60;
pub const DEFAULT_S3_ENDPOINT: &str = "localhost:9000";
pub const DEFAULT_S3_ACCESS_KEY: &str = "minioadmin";
pub const DEFAULT_S3_SECRET_KEY: &str = "minioadmin";
pub const DEFAULT_S3_BUCKET: &str = "minio-bucket";
pub const DEFAULT_CACHE_KEY_PREFIX: &str = "pilput";
pub const DEFAULT_CACHE_TTL_SECS: u64 = 60;
pub const DEFAULT_VALKEY_CONNECT_TIMEOUT_MS: i64 = 2000;
pub const DEFAULT_OPENROUTER_BASE_URL: &str = "https://openrouter.ai/api/v1";
pub const DEFAULT_OPENROUTER_DEFAULT_MODEL: &str = "openrouter/free";
pub const DEFAULT_OPENROUTER_HTTP_REFERER: &str = "https://pilput.net";
pub const DEFAULT_OPENROUTER_TITLE: &str = "pilput";
pub const DEFAULT_OPENROUTER_TIMEOUT_SECS: u64 = 90;
pub const DEFAULT_GITHUB_REDIRECT_URI: &str =
    "http://localhost:8080/api/auth/oauth/github/callback";
pub const DEFAULT_SMTP_PORT: i64 = 587;
pub const DEFAULT_SMTP_TIMEOUT_SECS: u64 = 10;
pub const DEFAULT_SMTP_TASK_TIMEOUT_SECS: u64 = 30;
pub const DEFAULT_HTTP_REQUEST_TIMEOUT_SECS: u64 = 30;

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
    pub openrouter: OpenRouterConfig,
    pub github: GitHubConfig,
    pub market: MarketConfig,
    /// Raw signed values kept only so `validate` can reject negatives the way
    /// echobackend does (the typed fields above are unsigned).
    pub checks: EnvChecks,
}

/// Raw integer settings echobackend validates as `>= 0` / `1..=65535`.
#[derive(Debug, Clone, Default)]
pub struct EnvChecks {
    pub cache_ttl_secs: i64,
    pub cache_connect_timeout_ms: i64,
    pub smtp_port: i64,
}

/// Database connection pool configuration
#[derive(Debug, Clone)]
pub struct PoolConfig {
    pub max_size: usize,
    pub min_idle: usize,
    pub connection_timeout: Duration,
    pub max_lifetime: Duration,
    pub idle_timeout: Duration,
}

/// JWT authentication configuration
#[derive(Debug, Clone)]
pub struct JwtConfig {
    pub secret: String,
    pub expiry: Duration,
    pub expiry_hours: i64,
    /// Sliding inactivity window of a session: every refresh moves
    /// `sessions.expires_at` to now + this value (REFRESH_TOKEN_EXPIRY,
    /// legacy REFRESH_TOKEN_EXPIRY_DAYS; default 72h like echobackend).
    pub refresh_token_expiry: Duration,
    /// Caps the total lifetime of a rotation chain, however often it is
    /// refreshed (OWASP ASVS v5.0 7.3.2).
    pub refresh_token_absolute_expiry: Duration,
    /// Keeps a just-rotated refresh token usable briefly so a client's own
    /// concurrent refreshes are not mistaken for a replay. Zero disables it.
    pub refresh_token_grace_period: Duration,
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

/// Global HTTP rate limit, mirroring echobackend's Echo `RateLimiter`: an
/// in-memory token bucket per client IP with `rate = rps` tokens/sec and
/// `burst = 2 * rps`. `rps == 0` disables it.
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// HTTP_RATE_LIMIT_RPS (aliases RATE_LIMITER_MAX, RATE_LIMIT_MAX_REQUESTS).
    /// Signed so `validate` can reject negatives like echobackend.
    pub rps: i64,
    /// How long an idle client's bucket is kept (HTTP_RATE_LIMIT_WINDOW_SEC,
    /// aliases RATE_LIMITER_TTL, RATE_LIMIT_WINDOW_SECS; 0 = 3 minutes). It is
    /// not a counting window.
    pub idle_expiry: Duration,
}

#[derive(Debug, Clone)]
pub struct HttpConfig {
    pub trust_proxy: bool,
    pub allow_origins: Vec<String>,
    pub request_timeout: Duration,
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
    /// RapidAPI key for financial data providers (IDX calendar, quotes).
    pub rapidapi_key: String,
    /// RapidAPI key specifically for the Indonesia Stock Exchange (IDX) corporate-actions API.
    pub rapidapi_idx_key: String,
}
