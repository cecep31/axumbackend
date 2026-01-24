use std::env;

pub struct Config {
    pub port: u16,
    pub database_url: String,
    pub pool_max_size: usize,
    pub pool_min_connections: usize,
}

impl Config {
    pub fn from_env() -> Self {
        let port = env::var("PORT")
            .unwrap_or_else(|_| "8000".to_string())
            .parse::<u16>()
            .expect("PORT must be a valid number");

        let database_url = env::var("DATABASE_URL").unwrap_or_else(|_| {
            "host=localhost user=postgres password=postgres dbname=rocketbackend".to_string()
        });

        let pool_max_size = env::var("POOL_MAX_SIZE")
            .unwrap_or_else(|_| "20".to_string())
            .parse::<usize>()
            .expect("POOL_MAX_SIZE must be a valid number");

        let pool_min_connections = env::var("POOL_MIN_CONNECTIONS")
            .unwrap_or_else(|_| "2".to_string())
            .parse::<usize>()
            .expect("POOL_MIN_CONNECTIONS must be a valid number");

        Config {
            port,
            database_url,
            pool_max_size,
            pool_min_connections,
        }
    }
}
