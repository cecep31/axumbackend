use crate::config::{GitHubConfig, JwtConfig};
use crate::entities::{auth_activity_logs, password_reset_tokens, sessions, users};
use crate::http::auth::Claims;
use crate::infra::cache;
use crate::infra::email::Mailer;
use argon2::{
    Argon2, Params,
    password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash},
};
use chrono::{DateTime, Duration, Utc};
use jsonwebtoken::{EncodingKey, Header, encode};
use rand::distr::{Alphanumeric, SampleString};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, DatabaseConnection, DbErr, EntityTrait, ExprTrait,
    PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, Set, TransactionTrait,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use uuid::Uuid;

mod account;
mod activity;
mod github;
mod password;
mod session;
mod types;

pub use account::*;
pub use activity::*;
pub use github::*;
pub use password::*;
pub use session::*;
pub use types::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_argon2id_hash_and_verify() {
        let password = "SecretPassword123!";
        let hashed = hash_password(password).expect("hashing should succeed");
        assert!(hashed.starts_with("$argon2id$"));

        let (valid, needs_rehash) = verify_password(&hashed, password);
        assert!(valid);
        assert!(!needs_rehash);

        let (invalid, _) = verify_password(&hashed, "WrongPassword");
        assert!(!invalid);
    }

    #[test]
    fn test_argon2id_outdated_params_need_rehash() {
        let password = "SecurePassword123!";
        let params = Params::new(19 * 1024, 2, 1, Some(32)).unwrap();
        let old_hash = Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params)
            .hash_password(password.as_bytes())
            .unwrap()
            .to_string();

        let (valid, needs_rehash) = verify_password(&old_hash, password);
        assert!(valid);
        assert!(needs_rehash);

        let (invalid, needs_rehash) = verify_password(&old_hash, "WrongPassword");
        assert!(!invalid);
        assert!(!needs_rehash);
    }

    #[test]
    fn test_token_hash_matches_echobackend() {
        // echobackend: hex(sha256(token))
        assert_eq!(
            token_hash("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn test_legacy_bcrypt_verify_and_rehash_flag() {
        let password = "LegacyBcryptPassword123!";
        let bcrypt_hash = bcrypt::hash(password, 4).expect("bcrypt hash should succeed");
        assert!(bcrypt_hash.starts_with("$2"));

        let (valid, needs_rehash) = verify_password(&bcrypt_hash, password);
        assert!(valid);
        assert!(needs_rehash);

        let (invalid, _) = verify_password(&bcrypt_hash, "WrongPassword");
        assert!(!invalid);
    }

    #[tokio::test]
    async fn test_oauth_exchange_roundtrip() {
        let token_resp = AuthTokenResponse {
            access_token: "access-123".into(),
            refresh_token: "pl_test".into(),
            user: UserBrief {
                id: Uuid::now_v7(),
                email: "test@example.com".into(),
                username: Some("testuser".into()),
            },
        };
        let code = create_oauth_exchange_code(token_resp).await;
        assert!(code.starts_with("oc_"));

        let redeemed = exchange_oauth_code(&code)
            .await
            .expect("should exchange successfully");
        assert_eq!(redeemed.access_token, "access-123");

        // Second exchange should fail (single-use)
        let second = exchange_oauth_code(&code).await;
        assert!(matches!(second, Err(AuthError::InvalidToken)));
    }
}
