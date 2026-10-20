//! S3-compatible object storage (RustFS, MinIO, AWS S3, …).
//!
//! Mirrors echobackend's `S3Storage`: path-style URLs, region `us-east-1`,
//! and an endpoint that may omit its scheme (then `S3_USE_SSL` picks it).
//! Requests are presigned with `rusty-s3` and sent with `reqwest`.

use crate::config::S3Config;
use reqwest::{Client, Url};
use rusty_s3::{Bucket, Credentials, S3Action, UrlStyle};
use std::sync::OnceLock;
use std::time::Duration;

const SAVE_TIMEOUT: Duration = Duration::from_secs(30);
/// Lifetime of the presigned URL; only needs to outlive the request itself.
const SIGN_EXPIRY: Duration = Duration::from_secs(60);
const REGION: &str = "us-east-1";

static STORAGE: OnceLock<Storage> = OnceLock::new();

pub struct Storage {
    bucket: Bucket,
    credentials: Credentials,
    client: Client,
}

#[derive(Debug)]
pub enum StorageError {
    Request(reqwest::Error),
    Status { status: u16, body: String },
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageError::Request(err) => write!(f, "storage request failed: {err}"),
            StorageError::Status { status, body } => {
                write!(f, "storage returned HTTP {status}: {body}")
            }
        }
    }
}

impl std::error::Error for StorageError {}

/// Returns the storage initialized by [`init`], or `None` when S3 is not
/// configured.
pub fn get() -> Option<&'static Storage> {
    STORAGE.get()
}

/// Builds the process-wide storage from config. Missing endpoint/bucket or an
/// invalid endpoint leaves storage disabled (uploads then report it).
pub fn init(cfg: &S3Config) -> Option<&'static Storage> {
    if cfg.endpoint.is_empty() || cfg.bucket.is_empty() {
        tracing::warn!("S3 configuration missing, storage disabled");
        return None;
    }

    let endpoint = endpoint_url(&cfg.endpoint, cfg.use_ssl);
    let bucket = match Url::parse(&endpoint)
        .map_err(|err| err.to_string())
        .and_then(|url| {
            Bucket::new(url, UrlStyle::Path, cfg.bucket.clone(), REGION)
                .map_err(|err| err.to_string())
        }) {
        Ok(bucket) => bucket,
        Err(err) => {
            tracing::warn!(error = %err, endpoint = %endpoint, "invalid S3 endpoint, storage disabled");
            return None;
        }
    };

    let client = match Client::builder().timeout(SAVE_TIMEOUT).build() {
        Ok(client) => client,
        Err(err) => {
            tracing::warn!(error = %err, "failed to build S3 HTTP client, storage disabled");
            return None;
        }
    };

    let storage = Storage {
        bucket,
        credentials: Credentials::new(cfg.access_key.clone(), cfg.secret_key.clone()),
        client,
    };
    if STORAGE.set(storage).is_err() {
        tracing::warn!("storage already initialized");
    }
    tracing::info!(endpoint = %endpoint, bucket = %cfg.bucket, "S3 storage enabled");
    STORAGE.get()
}

fn endpoint_url(endpoint: &str, use_ssl: bool) -> String {
    if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
        endpoint.to_string()
    } else {
        let scheme = if use_ssl { "https" } else { "http" };
        format!("{scheme}://{endpoint}")
    }
}

impl Storage {
    /// Uploads `data` to `key` with the given content type.
    pub async fn save(
        &self,
        key: &str,
        data: Vec<u8>,
        content_type: &str,
    ) -> Result<(), StorageError> {
        let mut action = self.bucket.put_object(Some(&self.credentials), key);
        action.headers_mut().insert("content-type", content_type);
        let url = action.sign(SIGN_EXPIRY);

        let response = self
            .client
            .put(url)
            .header(reqwest::header::CONTENT_TYPE, content_type)
            .body(data)
            .send()
            .await
            .map_err(StorageError::Request)?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(StorageError::Status {
                status: status.as_u16(),
                body,
            });
        }
        Ok(())
    }
}

/// Random object key under `prefix`, same shape as echobackend:
/// `<prefix>/<32 hex chars><ext>`.
pub fn random_object_key(prefix: &str, ext: &str) -> String {
    let bytes: [u8; 16] = rand::random();
    format!("{prefix}/{}{ext}", hex::encode(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_url_adds_scheme_from_use_ssl() {
        assert_eq!(
            endpoint_url("localhost:9000", false),
            "http://localhost:9000"
        );
        assert_eq!(
            endpoint_url("s3.example.com", true),
            "https://s3.example.com"
        );
        assert_eq!(endpoint_url("http://minio:9000", true), "http://minio:9000");
    }

    #[test]
    fn random_object_key_shape() {
        let key = random_object_key("posts/images", ".png");
        assert!(key.starts_with("posts/images/"));
        assert!(key.ends_with(".png"));
        assert_eq!(key.len(), "posts/images/".len() + 32 + ".png".len());
    }
}
