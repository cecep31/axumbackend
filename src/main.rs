use axumbackend::{config, database, handlers, rate_limit::RateLimiter};
use std::net::SocketAddr;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    // Initialize tracing subscriber for logging
    // Using registry() approach for better flexibility and composability
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                // Default to info level, with debug for tower_http to see request details
                // axum::rejection=trace enables showing rejection events at trace level
                format!(
                    "{}=info,tower_http=info,axum::rejection=trace",
                    env!("CARGO_CRATE_NAME")
                )
                .into()
            }),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = config::Config::from_env();
    config::JwtConfig::init(config.jwt.clone());
    config::EmailConfig::init(config.email.clone());

    // Create connection pool with configuration from environment
    let pool = database::create_pool(&config.database_url, &config.db_pool)
        .await
        .map_err(|e| {
            format!(
                "Failed to create database connection: {}. Check DATABASE_URL format",
                e
            )
        })?;
    tracing::info!(
        "Database connection pool created (max_size: {}, timeout: {:?})",
        config.db_pool.max_size,
        config.db_pool.connection_timeout
    );

    let limiter = if config.rate_limit.max_requests == 0 {
        tracing::info!("Rate limiter disabled");
        None
    } else {
        tracing::info!(
            "Rate limiter enabled (max_requests: {}, window: {:?})",
            config.rate_limit.max_requests,
            config.rate_limit.window
        );
        Some(RateLimiter::new(
            config.rate_limit.max_requests,
            config.rate_limit.window,
        ))
    };

    let app = handlers::create_router(limiter).with_state(pool);

    let addr = format!("0.0.0.0:{}", config.port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| format!("Failed to bind to {}: {}", addr, e))?;
    tracing::info!("Server listening on {}", addr);
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}
