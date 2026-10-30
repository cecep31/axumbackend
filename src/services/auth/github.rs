use super::*;

// ============================================================================
// GitHub OAuth
// ============================================================================

const GITHUB_AUTHORIZE_URL: &str = "https://github.com/login/oauth/authorize";
const GITHUB_ACCESS_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
const GITHUB_USER_URL: &str = "https://api.github.com/user";
const GITHUB_USER_EMAILS_URL: &str = "https://api.github.com/user/emails";
const OAUTH_EXCHANGE_TTL_MINUTES: i64 = 2;

/// GitHub user profile as returned by `GET https://api.github.com/user`.
/// Mirrors echobackend's `service.GithubUser`.
#[derive(Debug, Deserialize)]
pub struct GithubUser {
    pub login: String,
    pub id: i64,
    pub avatar_url: Option<String>,
    pub email: Option<String>,
    pub name: Option<String>,
    pub html_url: Option<String>,
}

/// Cached under `<prefix>:oauth_exchange:<code>` in echobackend's shape (its
/// untagged Go struct `oauthExchangeEntry{AccessToken, RefreshToken, User,
/// ExpiresAt}`), so a code minted by one backend can be redeemed by another
/// sharing Redis. Only `id`/`email`/`username` of `User` are read; the rest of
/// echobackend's `model.User` is ignored.
#[derive(Serialize, Deserialize)]
struct OAuthExchangeEntry {
    #[serde(rename = "AccessToken")]
    access_token: String,
    #[serde(rename = "RefreshToken")]
    refresh_token: String,
    #[serde(rename = "User", default)]
    user: Option<UserBrief>,
    #[serde(rename = "ExpiresAt")]
    expires_at: chrono::DateTime<Utc>,
}

impl OAuthExchangeEntry {
    fn new(response: AuthTokenResponse, expires_at: chrono::DateTime<Utc>) -> Self {
        Self {
            access_token: response.access_token,
            refresh_token: response.refresh_token,
            user: Some(response.user),
            expires_at,
        }
    }

    /// echobackend rejects an entry that is expired or has no user.
    fn redeem(self, now: chrono::DateTime<Utc>) -> Option<AuthTokenResponse> {
        if now > self.expires_at {
            return None;
        }
        Some(AuthTokenResponse {
            access_token: self.access_token,
            refresh_token: self.refresh_token,
            user: self.user?,
        })
    }
}

static OAUTH_EXCHANGE_CODES: OnceLock<Mutex<HashMap<String, OAuthExchangeEntry>>> = OnceLock::new();

fn oauth_exchange_store() -> &'static Mutex<HashMap<String, OAuthExchangeEntry>> {
    OAUTH_EXCHANGE_CODES.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(super) fn github_http_client() -> Result<reqwest::Client, AuthError> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(AuthError::Request)
}

/// Build the GitHub authorization URL for the OAuth redirect endpoint.
pub fn github_oauth_url(state: &str) -> String {
    let config = GitHubConfig::get();
    let mut url = reqwest::Url::parse(GITHUB_AUTHORIZE_URL).expect("GitHub authorize URL is valid");
    url.query_pairs_mut()
        .append_pair("client_id", &config.client_id)
        .append_pair("redirect_uri", &config.redirect_uri)
        .append_pair("scope", "user:email")
        .append_pair("state", state);
    url.to_string()
}

/// Exchange a GitHub authorization code for a GitHub access token.
pub async fn get_github_token(code: &str) -> Result<String, AuthError> {
    let config = GitHubConfig::get();
    let response = github_http_client()?
        .post(GITHUB_ACCESS_TOKEN_URL)
        .header(reqwest::header::ACCEPT, "application/json")
        .form(&[
            ("client_id", config.client_id.as_str()),
            ("client_secret", config.client_secret.as_str()),
            ("code", code),
            ("redirect_uri", config.redirect_uri.as_str()),
        ])
        .send()
        .await?;

    let body: serde_json::Value = response.json().await?;
    body.get("access_token")
        .and_then(|value| value.as_str())
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .ok_or_else(|| AuthError::OAuth("no access_token in GitHub response".to_string()))
}

/// Fetch the GitHub profile for the given GitHub access token.
pub async fn fetch_github_user(token: &str) -> Result<GithubUser, AuthError> {
    let response = github_http_client()?
        .get(GITHUB_USER_URL)
        .header(reqwest::header::AUTHORIZATION, format!("Bearer {token}"))
        .header(reqwest::header::ACCEPT, "application/vnd.github.v3+json")
        .header(reqwest::header::USER_AGENT, "pilput-axumbackend")
        .send()
        .await?;

    if response.status() != reqwest::StatusCode::OK {
        return Err(AuthError::OAuth(format!(
            "GitHub API returned status {}",
            response.status()
        )));
    }

    Ok(response.json::<GithubUser>().await?)
}

/// Fetch the primary email of the GitHub account, falling back to the first
/// email in the list. Mirrors echobackend's `fetchGithubUserEmail`.
pub async fn fetch_github_user_email(token: &str) -> Result<Option<String>, AuthError> {
    #[derive(Debug, Deserialize)]
    struct GithubEmail {
        email: String,
        primary: bool,
    }

    let response = github_http_client()?
        .get(GITHUB_USER_EMAILS_URL)
        .header(reqwest::header::AUTHORIZATION, format!("Bearer {token}"))
        .header(reqwest::header::ACCEPT, "application/vnd.github.v3+json")
        .header(reqwest::header::USER_AGENT, "pilput-axumbackend")
        .send()
        .await?;

    if response.status() != reqwest::StatusCode::OK {
        return Err(AuthError::OAuth(format!(
            "GitHub API returned status {}",
            response.status()
        )));
    }

    let emails: Vec<GithubEmail> = response.json().await?;
    if let Some(primary) = emails.iter().find(|entry| entry.primary) {
        return Ok(Some(primary.email.clone()));
    }
    Ok(emails.into_iter().next().map(|entry| entry.email))
}

/// Sign in (or register) a user from a GitHub profile and issue tokens.
/// Mirrors echobackend's `authService.SignInWithGithub`.
pub async fn sign_in_with_github(
    db: &DatabaseConnection,
    github_user: &GithubUser,
    client: ClientInfo,
) -> Result<AuthTokenResponse, AuthError> {
    let existing = users::Entity::find()
        .filter(users::Column::GithubId.eq(github_user.id))
        .filter(users::Column::DeletedAt.is_null())
        .one(db)
        .await?;

    let user = match existing {
        Some(user) => user,
        None => {
            // echobackend stores the normalized GitHub email, or the
            // `<id>@github.placeholder` fallback when there is none.
            let email = github_user
                .email
                .as_deref()
                .map(normalize_email)
                .filter(|email| !email.is_empty())
                .unwrap_or_else(|| format!("{}@github.placeholder", github_user.id));

            let new_user = users::ActiveModel {
                id: Set(Uuid::now_v7()),
                email: Set(email),
                username: Set(Some(github_user.login.clone())),
                github_id: Set(Some(github_user.id)),
                image: Set(github_user.avatar_url.clone()),
                created_at: Set(Some(Utc::now().into())),
                updated_at: Set(Some(Utc::now().into())),
                ..Default::default()
            };

            match new_user.insert(db).await {
                Ok(user) => user,
                Err(err) => {
                    log_activity(
                        db,
                        None,
                        "oauth_login_failed",
                        "failure",
                        client.ip_address.clone(),
                        client.user_agent.clone(),
                        None,
                        Some(serde_json::json!({
                            "provider": "github",
                            "error": err.to_string(),
                        })),
                    )
                    .await;
                    return Err(AuthError::Db(err));
                }
            }
        }
    };

    let response = match create_token_and_session(db, &user, client.clone()).await {
        Ok(response) => response,
        Err(err) => {
            log_activity(
                db,
                Some(user.id),
                "oauth_login_failed",
                "failure",
                client.ip_address.clone(),
                client.user_agent.clone(),
                None,
                Some(serde_json::json!({ "provider": "github" })),
            )
            .await;
            return Err(err);
        }
    };

    log_activity(
        db,
        Some(user.id),
        "oauth_login",
        "success",
        client.ip_address.clone(),
        client.user_agent.clone(),
        None,
        Some(serde_json::json!({ "provider": "github" })),
    )
    .await;

    let mut active_user: users::ActiveModel = user.clone().into();
    active_user.last_logged_at = Set(Some(Utc::now().into()));
    let _ = active_user.update(db).await;

    Ok(response)
}

pub(super) fn oauth_exchange_cache_key(cache: &cache::Cache, code: &str) -> String {
    cache.build_key(&["oauth_exchange", code])
}

/// Create a one-time OAuth exchange code (2-minute TTL) holding the issued
/// tokens. Mirrors echobackend's `CreateOAuthExchangeCode`: the code lives in
/// Redis (`oauth_exchange:<code>`) so any instance can redeem it, or in memory
/// when Redis is off or rejects the write.
pub async fn create_oauth_exchange_code(response: AuthTokenResponse) -> String {
    let code = generate_prefixed_token("oc");
    let now = Utc::now();
    let entry = OAuthExchangeEntry::new(
        response,
        now + Duration::minutes(OAUTH_EXCHANGE_TTL_MINUTES),
    );

    if let Some(cache) = cache::get() {
        let ttl = std::time::Duration::from_secs(OAUTH_EXCHANGE_TTL_MINUTES as u64 * 60);
        let key = oauth_exchange_cache_key(cache, &code);
        match cache.set_json_with_ttl(&key, &entry, ttl).await {
            Ok(()) => return code,
            Err(err) => {
                tracing::warn!(error = %err, "oauth exchange: Redis write failed, storing in memory");
            }
        }
    }

    let mut store = oauth_exchange_store()
        .lock()
        .expect("oauth exchange store lock poisoned");
    store.retain(|_, entry| entry.expires_at > now);
    store.insert(code.clone(), entry);
    code
}

/// Atomically redeem and delete a one-time OAuth exchange code.
/// Mirrors echobackend's `ExchangeOAuthCode`. Codes missing from Redis are
/// still looked up in memory, where they land while Redis is failing.
pub async fn exchange_oauth_code(code: &str) -> Result<AuthTokenResponse, AuthError> {
    if let Some(cache) = cache::get() {
        let key = oauth_exchange_cache_key(cache, code);
        match cache.take_json::<OAuthExchangeEntry>(&key).await {
            Ok(Some(entry)) => return entry.redeem(Utc::now()).ok_or(AuthError::InvalidToken),
            Ok(None) => {}
            Err(err) => {
                tracing::warn!(error = %err, "oauth exchange: Redis read failed, checking memory");
            }
        }
    }

    let now = Utc::now();
    let mut store = oauth_exchange_store()
        .lock()
        .expect("oauth exchange store lock poisoned");
    store.retain(|_, entry| entry.expires_at > now);
    store
        .remove(code)
        .and_then(|entry| entry.redeem(now))
        .ok_or(AuthError::InvalidToken)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_oauth_exchange_entry_reads_echobackend_shape() {
        let raw = r#"{
            "AccessToken": "at",
            "RefreshToken": "pl_rt",
            "User": {
                "id": "0190a8f2-1c3e-7b4a-9d6e-2f1a3b4c5d6e",
                "email": "user@example.com",
                "username": "user",
                "first_name": null,
                "created_at": "2026-01-01T00:00:00Z"
            },
            "ExpiresAt": "2999-01-01T00:00:00.123456789+07:00"
        }"#;
        let entry: OAuthExchangeEntry = serde_json::from_str(raw).expect("echo entry parses");
        let response = entry.redeem(Utc::now()).expect("not expired");
        assert_eq!(response.access_token, "at");
        assert_eq!(response.refresh_token, "pl_rt");
        assert_eq!(response.user.email, "user@example.com");
        assert_eq!(response.user.username.as_deref(), Some("user"));
    }

    #[test]
    fn test_oauth_exchange_entry_writes_echobackend_shape() {
        let entry = OAuthExchangeEntry::new(
            AuthTokenResponse {
                access_token: "at".into(),
                refresh_token: "pl_rt".into(),
                user: UserBrief {
                    id: Uuid::nil(),
                    email: "user@example.com".into(),
                    username: None,
                },
            },
            Utc::now(),
        );
        let value = serde_json::to_value(&entry).unwrap();
        for key in ["AccessToken", "RefreshToken", "User", "ExpiresAt"] {
            assert!(value.get(key).is_some(), "missing {key}");
        }
        assert_eq!(value["User"]["email"], "user@example.com");
    }

    #[test]
    fn test_oauth_exchange_entry_rejects_missing_user_or_expired() {
        let no_user: OAuthExchangeEntry = serde_json::from_str(
            r#"{"AccessToken":"a","RefreshToken":"r","User":null,"ExpiresAt":"2999-01-01T00:00:00Z"}"#,
        )
        .unwrap();
        assert!(no_user.redeem(Utc::now()).is_none());

        let expired: OAuthExchangeEntry = serde_json::from_str(
            r#"{"AccessToken":"a","RefreshToken":"r","User":{"id":"0190a8f2-1c3e-7b4a-9d6e-2f1a3b4c5d6e","email":"e","username":null},"ExpiresAt":"2000-01-01T00:00:00Z"}"#,
        )
        .unwrap();
        assert!(expired.redeem(Utc::now()).is_none());
    }
}
