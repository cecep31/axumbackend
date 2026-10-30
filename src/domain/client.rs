//! Who sent a request, as echobackend records it in `sessions`,
//! `auth_activity_logs` and `post_views`.

/// Resolved client IP (`c.RealIP()`, see `crate::http::client_ip`) and the
/// `User-Agent` header. Empty values are `None`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClientInfo {
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
}
