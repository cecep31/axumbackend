use crate::config::PoolConfig;
use sea_orm::{ConnectOptions, Database, DatabaseConnection, DbErr};

/// Shared database connection managed by SeaORM / SQLx internally.
pub type DbPool = DatabaseConnection;

/// Create a SeaORM database connection from the database URL and pool configuration.
pub async fn create_pool(
    database_url: &str,
    pool_config: &PoolConfig,
) -> Result<DatabaseConnection, DbErr> {
    let mut options = ConnectOptions::new(database_url.to_string());
    options
        .max_connections(pool_config.max_size as u32)
        .connect_timeout(pool_config.connection_timeout)
        .acquire_timeout(pool_config.connection_timeout);

    Database::connect(options).await
}
