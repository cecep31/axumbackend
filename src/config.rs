use std::env;
use std::time::Duration;

// ============================================================================
// Constants
// ============================================================================

/// Default server port
const DEFAULT_PORT: u16 = 8000;

/// Default database connection string
const DEFAULT_DATABASE_URL: &str =
    "host=localhost user=postgres password=postgres dbname=rocketbackend";

/// Default maximum number of connections in the pool
const DEFAULT_POOL_MAX_SIZE: usize = 20;

/// Default connection timeout in seconds
const DEFAULT_CONNECTION_TIMEOUT_SECS: u64 = 30;

/// Default maximum connection lifetime in seconds (30 minutes)
const DEFAULT_MAX_LIFETIME_SECS: u64 = 1800;

/// Default idle timeout in seconds (10 minutes)
const DEFAULT_IDLE_TIMEOUT_SECS: u64 = 600;

// ============================================================================
// Configuration Structures
// ============================================================================

/// Application configuration loaded from environment variables
#[derive(Debug, Clone)]
pub struct Config {
    /// Server port number
    pub port: u16,
    /// PostgreSQL database connection URL
    pub database_url: String,
    /// Database connection pool configuration
    pub db_pool: PoolConfig,
}

/// Database connection pool configuration
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// Maximum number of connections in the pool
    pub max_size: usize,
    /// Timeout for acquiring a connection from the pool
    pub connection_timeout: Duration,
    /// Maximum lifetime of a connection (None = no limit)
    /// Note: Reserved for future use with custom pool manager
    #[allow(dead_code)]
    pub max_lifetime: Option<Duration>,
    /// Idle timeout for connections (None = no limit)
    /// Note: Reserved for future use with custom pool manager
    #[allow(dead_code)]
    pub idle_timeout: Option<Duration>,
}

// ============================================================================
// Default Implementations
// ============================================================================

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_size: DEFAULT_POOL_MAX_SIZE,
            connection_timeout: Duration::from_secs(DEFAULT_CONNECTION_TIMEOUT_SECS),
            max_lifetime: Some(Duration::from_secs(DEFAULT_MAX_LIFETIME_SECS)),
            idle_timeout: Some(Duration::from_secs(DEFAULT_IDLE_TIMEOUT_SECS)),
        }
    }
}

// ============================================================================
// Configuration Loading
// ============================================================================

impl Config {
    /// Load configuration from environment variables with sensible defaults
    ///
    /// # Environment Variables
    /// - `PORT`: Server port (default: 8000)
    /// - `DATABASE_URL`: PostgreSQL connection string
    /// - `DB_POOL_MAX_SIZE`: Maximum pool size (default: 20)
    /// - `DB_POOL_CONNECTION_TIMEOUT`: Connection timeout in seconds (default: 30)
    /// - `DB_POOL_MAX_LIFETIME`: Max connection lifetime in seconds, 0 = no limit (default: 1800)
    /// - `DB_POOL_IDLE_TIMEOUT`: Idle timeout in seconds, 0 = no limit (default: 600)
    ///
    /// # Panics
    /// Panics if required numeric values cannot be parsed as valid numbers.
    pub fn from_env() -> Self {
        Self {
            port: parse_port(),
            database_url: parse_database_url(),
            db_pool: PoolConfig::from_env(),
        }
    }
}

impl PoolConfig {
    /// Load pool configuration from environment variables
    fn from_env() -> Self {
        Self {
            max_size: parse_usize_env("DB_POOL_MAX_SIZE", DEFAULT_POOL_MAX_SIZE),
            connection_timeout: Duration::from_secs(parse_u64_env(
                "DB_POOL_CONNECTION_TIMEOUT",
                DEFAULT_CONNECTION_TIMEOUT_SECS,
            )),
            max_lifetime: parse_optional_duration(
                "DB_POOL_MAX_LIFETIME",
                DEFAULT_MAX_LIFETIME_SECS,
            ),
            idle_timeout: parse_optional_duration(
                "DB_POOL_IDLE_TIMEOUT",
                DEFAULT_IDLE_TIMEOUT_SECS,
            ),
        }
    }
}

// ============================================================================
// Environment Variable Parsing Helpers
// ============================================================================

/// Parse server port from environment variable
fn parse_port() -> u16 {
    parse_u16_env("PORT", DEFAULT_PORT)
}

/// Parse database URL from environment variable
fn parse_database_url() -> String {
    env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string())
}

/// Parse a u16 from environment variable with default fallback
fn parse_u16_env(key: &str, default: u16) -> u16 {
    env::var(key)
        .unwrap_or_else(|_| default.to_string())
        .parse::<u16>()
        .unwrap_or_else(|_| panic!("{key} must be a valid u16 number (0-65535), got invalid value"))
}

/// Parse a u64 from environment variable with default fallback
fn parse_u64_env(key: &str, default: u64) -> u64 {
    env::var(key)
        .unwrap_or_else(|_| default.to_string())
        .parse::<u64>()
        .unwrap_or_else(|_| panic!("{key} must be a valid u64 number, got invalid value"))
}

/// Parse a usize from environment variable with default fallback
fn parse_usize_env(key: &str, default: usize) -> usize {
    env::var(key)
        .unwrap_or_else(|_| default.to_string())
        .parse::<usize>()
        .unwrap_or_else(|_| panic!("{key} must be a valid usize number, got invalid value"))
}

/// Parse an optional duration from environment variable
///
/// # Behavior
/// - If env var is "0", returns `None` (no limit)
/// - If env var is not set, uses `default_secs`
/// - If env var is set to a valid number, returns `Some(Duration::from_secs(value))`
/// - If env var is set to invalid value, uses `default_secs`
fn parse_optional_duration(env_key: &str, default_secs: u64) -> Option<Duration> {
    let value = env::var(env_key)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(default_secs);

    match value {
        0 => None,
        secs => Some(Duration::from_secs(secs)),
    }
}
