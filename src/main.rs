mod database;
mod models;
mod routes;
mod services;

use axum::{Router, routing::get};
use routes::health::health;
use routes::post::{get_posts, get_random_posts};
use std::sync::Arc;
use tower_http::cors::CorsLayer;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let db_conn = database::connect()
        .await
        .expect("failed to connect to database");

    let app = Router::new()
        .route("/v1/health", get(health))
        .route("/v1/posts", get(get_posts))
        .route("/v1/posts/random", get(get_random_posts))
        .with_state(Arc::new(db_conn))
        .layer(CorsLayer::permissive());

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
    println!("Server listening on 0.0.0.0:8001");
}
