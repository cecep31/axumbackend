#[macro_use]
extern crate rocket;

mod database;
mod models;
mod routes;
mod services;

use std::sync::Arc;
use routes::health::health;
use routes::post::get_posts;

#[launch]
async fn rocket() -> _ {
    // Load environment variables from .env file
    dotenvy::dotenv().ok();
    
    let db_conn = database::connect().await.expect("failed to connect to database");

    rocket::build()
        .manage(Arc::new(db_conn))
        .mount("/", routes![health, get_posts])
}
