use deadpool_postgres::{Config, Pool, Runtime};
use tokio_postgres::NoTls;

pub type DbPool = Pool;

/// Create a connection pool from the database URL
/// 
/// Default pool configuration:
/// - max_size: 16 connections (CPU count * 4)
/// - Connections are created lazily on demand
/// - Connections are automatically recycled when returned to pool
pub fn create_pool(database_url: &str) -> Pool {
    let mut cfg = Config::new();
    cfg.url = Some(database_url.to_string());
    cfg.create_pool(Some(Runtime::Tokio1), NoTls)
        .expect("Failed to create database pool")
}
