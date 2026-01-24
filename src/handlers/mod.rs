mod health;
mod post;
mod tag;

use axum::Router;
use std::sync::Arc;
use tokio_postgres::Client;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

pub fn create_router() -> Router<Arc<Client>> {
    Router::new()
        .merge(health::routes())
        .merge(post::routes())
        .merge(tag::routes())
        // TraceLayer should be added early to trace all requests
        // It provides good defaults: logs method, uri, status, latency automatically
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
}
