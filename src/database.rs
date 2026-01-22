use tokio_postgres::{Client, NoTls, Error};

pub async fn connect(database_url: &str) -> Result<Client, Error> {
    let (client, connection) = tokio_postgres::connect(database_url, NoTls).await?;

    // Spawn the connection handling in the background
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!("connection error: {}", e);
        }
    });

    Ok(client)
}
