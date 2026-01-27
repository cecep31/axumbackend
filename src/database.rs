use crate::config::PoolConfig;
use deadpool_postgres::{Config, CreatePoolError, Pool, Runtime};
use tokio_postgres::NoTls;

/// Type alias for the database connection pool
pub type DbPool = Pool;

/// Create a connection pool from the database URL and pool configuration
///
/// # Pool Configuration
/// - `max_size`: Maximum number of connections in the pool
/// - `connection_timeout`: Timeout for acquiring/creating/recycling connections
///
/// # Errors
/// Returns `CreatePoolError` if pool creation fails (e.g., invalid URL format)
pub fn create_pool(database_url: &str, pool_config: &PoolConfig) -> Result<Pool, CreatePoolError> {
    let mut cfg = Config::new();
    cfg.url = Some(database_url.to_string());

    cfg.pool = Some(deadpool_postgres::PoolConfig {
        max_size: pool_config.max_size,
        timeouts: deadpool::managed::Timeouts {
            wait: Some(pool_config.connection_timeout),
            create: Some(pool_config.connection_timeout),
            recycle: Some(pool_config.connection_timeout),
        },
        ..Default::default()
    });

    cfg.create_pool(Some(Runtime::Tokio1), NoTls)
}
