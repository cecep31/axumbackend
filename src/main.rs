mod config;
mod database;
mod error;
mod handlers;
mod models;
mod services;

use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() {
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

    let db_conn = database::connect(&config.database_url)
        .await
        .expect("failed to connect to database");

    let app = handlers::create_router().with_state(Arc::new(db_conn));

    let addr = format!("0.0.0.0:{}", config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    tracing::info!("Server listening on {}", addr);
    axum::serve(listener, app).await.unwrap();
}
