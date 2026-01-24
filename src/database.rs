use deadpool_postgres::{Config, ManagerConfig, Pool, RecyclingMethod, Runtime};
use tokio_postgres::NoTls;

pub fn create_pool(database_url: &str, max_size: usize) -> Pool {
    let mut cfg = Config::new();
    
    // Parse connection string and set config
    cfg.url = Some(database_url.to_string());
    cfg.manager = Some(ManagerConfig {
        recycling_method: RecyclingMethod::Fast,
    });
    cfg.pool = Some(deadpool_postgres::PoolConfig {
        max_size,
        timeouts: Default::default(),
        queue_mode: Default::default(),
    });

    cfg.create_pool(Some(Runtime::Tokio1), NoTls)
        .expect("Failed to create pool")
}

/// Pre-warm the pool by creating connections upfront
/// This eliminates cold-start latency for initial requests
pub async fn warm_pool(pool: &Pool, count: usize) {
    let warm_count = count.min(pool.status().max_size);
    tracing::info!("Warming up pool with {} connections...", warm_count);
    
    let mut handles = Vec::with_capacity(warm_count);
    
    for _ in 0..warm_count {
        let pool = pool.clone();
        handles.push(tokio::spawn(async move {
            match pool.get().await {
                Ok(conn) => {
                    // Execute simple query to ensure connection is ready
                    let _ = conn.query_one("SELECT 1", &[]).await;
                    true
                }
                Err(e) => {
                    tracing::warn!("Failed to warm connection: {}", e);
                    false
                }
            }
        }));
    }
    
    let mut success = 0;
    for handle in handles {
        if let Ok(true) = handle.await {
            success += 1;
        }
    }
    
    tracing::info!("Pool warmed: {}/{} connections ready", success, warm_count);
}
