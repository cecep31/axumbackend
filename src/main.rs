mod config;
mod controllers;
mod database;
mod error;
mod models;
mod services;

use axum::{Router, routing::get};
use controllers::health::health;
use controllers::post::{get_posts, get_random_posts};
use std::sync::Arc;
use tower_http::cors::CorsLayer;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let config = config::Config::from_env();

    let db_conn = database::connect(&config.database_url)
        .await
        .expect("failed to connect to database");

    let app = Router::new()
    .route("/", get(health))
        .route("/v1/health", get(health))
        .route("/v1/posts", get(get_posts))
        .route("/v1/posts/random", get(get_random_posts))
        .with_state(Arc::new(db_conn))
        .layer(CorsLayer::permissive());

    let addr = format!("0.0.0.0:{}", config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    println!("Server listening on {}", addr);
    axum::serve(listener, app).await.unwrap();
}
