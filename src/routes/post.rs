use crate::models::post::Post;
use crate::services;
use std::sync::Arc;
use rocket::serde::json::Json;
use rocket::State;
use tokio_postgres::Client;

#[get("/posts")]
pub async fn get_posts(conn: &State<Arc<Client>>) -> Json<Vec<Post>> {
    let posts = services::post::get_all_posts(&conn.inner()).await.unwrap_or_else(|_| vec![]);
    Json(posts)
}

#[get("/posts/random?<limit>")]
pub async fn get_random_posts(conn: &State<Arc<Client>>, limit: Option<i64>) -> Json<Vec<Post>> {
    let limit = limit.unwrap_or(9);
    let posts = services::post::get_random_posts(&conn.inner(), limit).await.unwrap_or_else(|_| vec![]);
    Json(posts)
}
