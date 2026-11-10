//! Two-step account deletion, mirroring echobackend's
//! `AuthService.RequestAccountDeletion` / `ConfirmAccountDeletion`: the
//! authenticated request only emails a one-time link, and the account is
//! soft-deleted when that link's token is confirmed.

use super::*;
use crate::config::{DEFAULT_FRONTEND_DELETE_ACCOUNT_URL, FrontendConfig};
use crate::entities::account_deletion_tokens;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

/// echobackend `model.ActivityAccountDeletionReq`.
pub const ACTIVITY_ACCOUNT_DELETION_REQUEST: &str = "account_deletion_request";
/// echobackend `model.ActivityAccountDeleted`.
pub const ACTIVITY_ACCOUNT_DELETED: &str = "account_deleted";

/// How long the link in the account deletion email works (echobackend
/// `accountDeletionTTL`). The email text says "30 minutes".
pub(super) const ACCOUNT_DELETION_TTL_MINUTES: i64 = 30;

/// echobackend: `"ad_" + base64.RawURLEncoding(32 random bytes)`.
pub(super) fn generate_account_deletion_token() -> String {
    let bytes: [u8; 32] = rand::random();
    format!("ad_{}", URL_SAFE_NO_PAD.encode(bytes))
}

fn build_account_deletion_link(token: &str) -> String {
    token_link_with_base(
        &FrontendConfig::get().delete_account_url,
        DEFAULT_FRONTEND_DELETE_ACCOUNT_URL,
        token,
    )
}

/// Emails the user a one-time link that confirms deleting their account.
/// Nothing is deleted yet; a new request replaces any earlier link.
///
/// Errors: [`AuthError::EmailNotConfigured`] (checked before anything is
/// written), [`AuthError::UserNotFound`], [`AuthError::Email`] when the email
/// cannot be queued, or a database error.
pub async fn request_account_deletion(
    db: &DatabaseConnection,
    mailer: &dyn Mailer,
    user_id: Uuid,
    client: ClientInfo,
) -> Result<(), AuthError> {
    if !mailer.is_configured() {
        return Err(AuthError::EmailNotConfigured);
    }

    let user = users::Entity::find_by_id(user_id)
        .filter(users::Column::DeletedAt.is_null())
        .one(db)
        .await?
        .ok_or(AuthError::UserNotFound)?;

    let token = generate_account_deletion_token();

    // Best effort, as in echobackend: only the newest link stays valid.
    if let Err(err) = account_deletion_tokens::Entity::delete_many()
        .filter(account_deletion_tokens::Column::UserId.eq(user.id))
        .exec(db)
        .await
    {
        tracing::warn!(?err, user_id = %user.id, "failed to delete previous account deletion tokens");
    }

    // Only the SHA-256 digest is stored (echobackend `tokenHash`).
    let now = Utc::now();
    account_deletion_tokens::ActiveModel {
        id: Set(Uuid::now_v7()),
        user_id: Set(user.id),
        token: Set(token_hash(&token)),
        created_at: Set(now.into()),
        expires_at: Set((now + Duration::minutes(ACCOUNT_DELETION_TTL_MINUTES)).into()),
        used_at: Set(None),
    }
    .insert(db)
    .await?;

    let link = build_account_deletion_link(&token);
    if let Err(err) = mailer.enqueue_account_deletion_email(&user.email, &link) {
        log_activity(
            db,
            Some(user.id),
            ACTIVITY_ACCOUNT_DELETION_REQUEST,
            "failure",
            client.ip_address.clone(),
            client.user_agent.clone(),
            Some("Failed to queue email".to_string()),
            None,
        )
        .await;
        return Err(AuthError::Email(format!(
            "queue account deletion email: {err}"
        )));
    }

    log_activity(
        db,
        Some(user.id),
        ACTIVITY_ACCOUNT_DELETION_REQUEST,
        "success",
        client.ip_address.clone(),
        client.user_agent.clone(),
        None,
        None,
    )
    .await;
    Ok(())
}

/// Soft-deletes the account owning `token` (from the deletion email) and
/// revokes all of its sessions. A token works once.
///
/// Errors, checked in echobackend's order: [`AuthError::InvalidToken`]
/// (unknown token, or the user is already gone), [`AuthError::TokenUsed`],
/// [`AuthError::TokenExpired`], or a database error.
pub async fn confirm_account_deletion(
    db: &DatabaseConnection,
    token: &str,
    client: ClientInfo,
) -> Result<(), AuthError> {
    let entry = account_deletion_tokens::Entity::find()
        .filter(account_deletion_tokens::Column::Token.eq(token_hash(token)))
        .one(db)
        .await?
        .ok_or(AuthError::InvalidToken)?;

    if entry.used_at.is_some() {
        return Err(AuthError::TokenUsed);
    }
    if entry.expires_at.with_timezone(&Utc) < Utc::now() {
        return Err(AuthError::TokenExpired);
    }

    // Spend the token before deleting: the conditional UPDATE lets only one
    // of two concurrent requests with the same link through. Both writes share
    // a transaction so a failed delete does not burn the link.
    let now = Utc::now();
    let txn = db.begin().await?;
    let claimed = account_deletion_tokens::Entity::update_many()
        .col_expr(
            account_deletion_tokens::Column::UsedAt,
            sea_orm::sea_query::Expr::value(now),
        )
        .filter(account_deletion_tokens::Column::Id.eq(entry.id))
        .filter(account_deletion_tokens::Column::UsedAt.is_null())
        .exec(&txn)
        .await?;
    if claimed.rows_affected == 0 {
        return Err(AuthError::TokenUsed);
    }

    // Same soft delete as the former immediate `DELETE /api/auth/account`.
    let deleted = users::Entity::update_many()
        .col_expr(
            users::Column::DeletedAt,
            sea_orm::sea_query::Expr::value(now),
        )
        .col_expr(
            users::Column::UpdatedAt,
            sea_orm::sea_query::Expr::value(now),
        )
        .filter(users::Column::Id.eq(entry.user_id))
        .filter(users::Column::DeletedAt.is_null())
        .exec(&txn)
        .await?;
    if deleted.rows_affected == 0 {
        return Err(AuthError::InvalidToken);
    }
    txn.commit().await?;

    if let Err(err) = sessions::Entity::delete_many()
        .filter(sessions::Column::UserId.eq(entry.user_id))
        .exec(db)
        .await
    {
        tracing::warn!(?err, user_id = %entry.user_id, "failed to invalidate sessions after account deletion");
    }

    log_activity(
        db,
        Some(entry.user_id),
        ACTIVITY_ACCOUNT_DELETED,
        "success",
        client.ip_address.clone(),
        client.user_agent.clone(),
        None,
        None,
    )
    .await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    /// Records queued deletion emails; `configured` mirrors `IsConfigured`.
    #[derive(Default)]
    struct FakeMailer {
        configured: bool,
        sent: StdMutex<Vec<(String, String)>>,
    }

    impl Mailer for FakeMailer {
        fn is_configured(&self) -> bool {
            self.configured
        }
        fn enqueue_password_reset_email(&self, _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn enqueue_account_deletion_email(&self, to: &str, link: &str) -> Result<(), String> {
            self.sent
                .lock()
                .unwrap()
                .push((to.to_string(), link.to_string()));
            Ok(())
        }
    }

    fn client() -> ClientInfo {
        ClientInfo {
            ip_address: None,
            user_agent: None,
        }
    }

    #[test]
    fn token_format_matches_echobackend() {
        let token = generate_account_deletion_token();
        let encoded = token.strip_prefix("ad_").expect("ad_ prefix");
        // 32 bytes as unpadded base64url.
        assert_eq!(encoded.len(), 43);
        assert!(
            encoded
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
            "{token}"
        );
        assert_eq!(URL_SAFE_NO_PAD.decode(encoded).unwrap().len(), 32);
        assert_ne!(token, generate_account_deletion_token());
        // Stored hashed, never raw.
        assert_ne!(token_hash(&token), token);
        assert_eq!(token_hash(&token).len(), 64);
    }

    #[test]
    fn deletion_link_sets_token_query_param() {
        assert_eq!(
            token_link_with_base(
                "https://pilput.example/confirm-delete-account",
                DEFAULT_FRONTEND_DELETE_ACCOUNT_URL,
                "ad_abc-_x",
            ),
            "https://pilput.example/confirm-delete-account?token=ad_abc-_x"
        );
        assert_eq!(
            token_link_with_base("", DEFAULT_FRONTEND_DELETE_ACCOUNT_URL, "ad_1"),
            "http://localhost:3000/confirm-delete-account?token=ad_1"
        );
        assert_eq!(
            token_link_with_base("https://x.example/d?token=old&a=1", "", "ad_2"),
            "https://x.example/d?a=1&token=ad_2"
        );
    }

    /// echobackend `TestRequestAccountDeletion_EmailNotConfigured`: the check
    /// runs before any database access, so no token is created.
    #[tokio::test]
    async fn request_without_email_delivery_touches_nothing() {
        // Disconnected: any query would panic, so this proves none is made.
        let db = DatabaseConnection::default();
        let mailer = FakeMailer::default();
        let err = request_account_deletion(&db, &mailer, Uuid::now_v7(), client())
            .await
            .unwrap_err();
        assert!(matches!(err, AuthError::EmailNotConfigured), "{err:?}");
        assert!(mailer.sent.lock().unwrap().is_empty());
    }
}
