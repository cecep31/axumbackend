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
