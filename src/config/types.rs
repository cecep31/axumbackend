use std::time::Duration;

// ============================================================================
// Constants
// ============================================================================

pub const DEFAULT_PORT: u16 = 8080;
pub const DEFAULT_DATABASE_URL: &str = "postgresql://postgres:postgres@localhost:5432/axumbackend";
pub const DEFAULT_POOL_MAX_SIZE: usize = 20;
pub const DEFAULT_CONNECTION_TIMEOUT_SECS: u64 = 30;
pub const DEFAULT_JWT_SECRET: &str = "your-secret-key";
pub const DEFAULT_JWT_EXPIRY_HOURS: i64 = 3;
pub const DEFAULT_REFRESH_TOKEN_EXPIRY_DAYS: i64 = 30;
pub const DEFAULT_EMAIL_FROM: &str = "noreply@pilput.net";
pub const DEFAULT_FRONTEND_URL: &str = "http://localhost:3000";
pub const DEFAULT_FRONTEND_OAUTH_CALLBACK_URL: &str = "http://localhost:3000/auth/callback";
pub const DEFAULT_FRONTEND_RESET_PASSWORD_URL: &str = "http://localhost:3000/reset-password";
pub const DEFAULT_MAIN_DOMAIN: &str = "localhost";
pub const DEFAULT_RATE_LIMIT_MAX_REQUESTS: u32 = 0;
pub const DEFAULT_RATE_LIMIT_WINDOW_SECS: u64 = 60;
pub const DEFAULT_S3_ENDPOINT: &str = "localhost:9000";
pub const DEFAULT_S3_ACCESS_KEY: &str = "minioadmin";
pub const DEFAULT_S3_SECRET_KEY: &str = "minioadmin";
pub const DEFAULT_S3_BUCKET: &str = "minio-bucket";
pub const DEFAULT_CACHE_KEY_PREFIX: &str = "pilput";
pub const DEFAULT_CACHE_TTL_SECS: u64 = 60;
pub const DEFAULT_VALKEY_CONNECT_TIMEOUT_MS: u64 = 5000;
pub const DEFAULT_QUEUE_DEFAULT_NAME: &str = "default";
pub const DEFAULT_QUEUE_CONCURRENCY: u32 = 1;
pub const DEFAULT_QUEUE_MAX_RETRY: u32 = 5;
pub const DEFAULT_OPENROUTER_BASE_URL: &str = "https://openrouter.ai/api/v1";
pub const DEFAULT_OPENROUTER_DEFAULT_MODEL: &str = "openrouter/free";
pub const DEFAULT_OPENROUTER_HTTP_REFERER: &str = "https://pilput.net";
pub const DEFAULT_OPENROUTER_TITLE: &str = "pilput";
pub const DEFAULT_OPENROUTER_TIMEOUT_SECS: u64 = 90;
pub const DEFAULT_GITHUB_REDIRECT_URI: &str = "http://localhost:8080/api/auth/oauth/github/callback";
pub const DEFAULT_SMTP_PORT: u16 = 587;
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
