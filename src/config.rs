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
const DEFAULT_EMAIL_FROM: &str = "noreply@pilput.net";
const DEFAULT_FRONTEND_RESET_PASSWORD_URL: &str = "http://localhost:3000/reset-password";

// ============================================================================
// Configuration Structures
// ============================================================================

/// Application configuration loaded from environment variables
#[derive(Debug, Clone)]
pub struct Config {
    pub port: u16,
    pub database_url: String,
    pub db_pool: PoolConfig,
    pub jwt: JwtConfig,
    pub email: EmailConfig,
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
}

/// Email delivery configuration
#[derive(Debug, Clone)]
pub struct EmailConfig {
    pub resend_api_key: String,
    pub from: String,
    pub frontend_reset_password_url: String,
}

static JWT_CONFIG: OnceLock<JwtConfig> = OnceLock::new();
static EMAIL_CONFIG: OnceLock<EmailConfig> = OnceLock::new();

impl JwtConfig {
    fn from_env() -> Self {
        Self {
            secret: env::var("JWT_SECRET").unwrap_or_else(|_| DEFAULT_JWT_SECRET.to_string()),
            expiry_hours: parse_i64("JWT_EXPIRY_HOURS", DEFAULT_JWT_EXPIRY_HOURS),
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
            from: env::var("EMAIL_FROM").unwrap_or_else(|_| DEFAULT_EMAIL_FROM.to_string()),
            frontend_reset_password_url: env::var("FRONTEND_RESET_PASSWORD_URL")
                .unwrap_or_else(|_| DEFAULT_FRONTEND_RESET_PASSWORD_URL.to_string()),
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
    ///
    /// # Panics
    /// Panics if numeric values cannot be parsed.
    pub fn from_env() -> Self {
        Self {
            port: parse_u16("PORT", DEFAULT_PORT),
            database_url: env::var("DATABASE_URL")
                .unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string()),
            db_pool: PoolConfig::from_env(),
            jwt: JwtConfig::from_env(),
            email: EmailConfig::from_env(),
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
