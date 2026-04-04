use std::env;
use std::time::Duration;

// ============================================================================
// Constants
// ============================================================================

const DEFAULT_PORT: u16 = 8080;
const DEFAULT_DATABASE_URL: &str =
    "host=localhost user=postgres password=postgres dbname=axumbackend";
const DEFAULT_POOL_MAX_SIZE: usize = 20;
const DEFAULT_CONNECTION_TIMEOUT_SECS: u64 = 30;

// ============================================================================
// Configuration Structures
// ============================================================================

/// Application configuration loaded from environment variables
#[derive(Debug, Clone)]
pub struct Config {
    pub port: u16,
    pub database_url: String,
    pub db_pool: PoolConfig,
}

/// Database connection pool configuration
#[derive(Debug, Clone)]
pub struct PoolConfig {
    pub max_size: usize,
    pub connection_timeout: Duration,
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
    ///
    /// # Panics
    /// Panics if numeric values cannot be parsed.
    pub fn from_env() -> Self {
        Self {
            port: parse_u16("PORT", DEFAULT_PORT),
            database_url: env::var("DATABASE_URL")
                .unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string()),
            db_pool: PoolConfig::from_env(),
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
