use axumbackend::{
    config, handlers,
    http::rate_limit::TokenBucketLimiter,
    infra::{cache, database, email::QueuedMailer, realtime, storage},
    state::AppState,
};
use std::{future::IntoFuture, net::SocketAddr, sync::Arc, time::Duration};
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
    config
        .validate()
        .map_err(|e| format!("Invalid configuration: {}", e))?;

    config.init_globals();

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
        "Database connection pool created (max_size: {}, min_idle: {}, timeout: {:?})",
        config.db_pool.max_size,
        config.db_pool.min_idle,
        config.db_pool.connection_timeout
    );

    // Optional: without Redis, caching, auth rate limits, OAuth exchange codes
    // and realtime delivery stay in-process.
    if let Some(cache) = cache::init(&config.cache).await {
        realtime::hub().start_relay(cache);
    }

    // Optional: without S3, image uploads report "Storage is not configured".
    storage::init(&config.s3);

    // Global HTTP limiter, like echobackend: token bucket per client IP
    // (rate = HTTP_RATE_LIMIT_RPS, burst = 2x), 0 = disabled.
    let limiter = if config.rate_limit.rps <= 0 {
        tracing::info!("Rate limiter disabled");
        None
    } else {
        let rps = u32::try_from(config.rate_limit.rps).unwrap_or(u32::MAX);
        tracing::info!(
            "Rate limiter enabled (rps: {}, burst: {}, idle_expiry: {:?})",
            rps,
            u64::from(rps) * 2,
            config.rate_limit.idle_expiry
        );
        Some(TokenBucketLimiter::new(
            rps,
            config.rate_limit.idle_expiry,
            config.http.trust_proxy,
        ))
    };

    let state = AppState {
        db: pool,
        // SMTP (echobackend) when SMTP_HOST is set, else Resend when
        // RESEND_API_KEY is set, else dev mode.
        mailer: Arc::new(QueuedMailer::from_config(&config.email)),
    };
    let app = handlers::create_router(limiter).with_state(state);

    let addr = format!("0.0.0.0:{}", config.port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| format!("Failed to bind to {}: {}", addr, e))?;
    tracing::info!("Server listening on {}", addr);
    // echobackend `main`: on SIGINT/SIGTERM stop accepting connections, end
    // every SSE stream (the hub closes all subscriptions so clients reconnect
    // elsewhere) and give in-flight requests at most 10s to finish.
    let shutdown_started = Arc::new(tokio::sync::Notify::new());
    let on_shutdown = shutdown_started.clone();
    let server = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        shutdown_signal().await;
        tracing::info!("server is shutting down");
        realtime::hub().close();
        on_shutdown.notify_one();
    })
    .into_future();
    tokio::pin!(server);

    tokio::select! {
        result = &mut server => result?,
        _ = shutdown_started.notified() => {
            match tokio::time::timeout(SHUTDOWN_TIMEOUT, &mut server).await {
                Ok(result) => result?,
                Err(_) => tracing::error!("server forced to shutdown after {SHUTDOWN_TIMEOUT:?}"),
            }
        }
    }
    Ok(())
}

/// Upper bound on draining in-flight requests (echobackend's 10s
/// `server.Shutdown` context).
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(10);

/// Resolves on Ctrl-C (SIGINT) or, on Unix, SIGTERM.
async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(err) = tokio::signal::ctrl_c().await {
            tracing::error!(error = %err, "failed to listen for Ctrl-C");
            std::future::pending::<()>().await;
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(err) => {
                tracing::error!(error = %err, "failed to listen for SIGTERM");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
