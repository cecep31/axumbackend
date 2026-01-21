use tokio_postgres::{Client, NoTls, Error};
use std::env;

pub async fn connect() -> Result<Client, Error> {
    let database_url = env::var("DATABASE_URL")
        .unwrap_or_else(|_| "host=localhost user=postgres password=postgres dbname=rocketbackend".to_string());

    let (client, connection) = tokio_postgres::connect(&database_url, NoTls).await?;

    // Spawn the connection handling in the background
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!("connection error: {}", e);
        }
    });

    // Create the posts table if it doesn't exist
    client.execute(
        "CREATE TABLE IF NOT EXISTS posts (
            id SERIAL PRIMARY KEY,
            title VARCHAR(255) NOT NULL,
            body TEXT NOT NULL,
            published_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
        )",
        &[],
    ).await?;

    Ok(client)
}
