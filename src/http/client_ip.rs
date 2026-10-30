//! Client IP resolution, mirroring echobackend's `c.RealIP()`.
//!
//! echobackend installs `echo.ExtractIPDirect()` by default and
//! `echo.ExtractIPFromXFFHeader()` when `HTTP_TRUST_PROXY` is set. The latter
//! appends the direct peer to the `X-Forwarded-For` hops and walks them right
//! to left, returning the first hop that is not a trusted proxy
//! (loopback/private/link-local; here also CGNAT `100.64.0.0/10`, which is
//! what Railway's edge proxies connect from). An unparsable hop stops the walk
//! and yields the direct peer, so a client can only influence the result when
//! it really arrives through a trusted proxy.

use std::net::{IpAddr, SocketAddr};

use axum::{
    extract::{ConnectInfo, FromRequestParts},
    http::{HeaderMap, header, request::Parts},
};

use crate::config::HttpConfig;
use crate::domain::client::ClientInfo;

/// Whether `ip` is a proxy hop that may set `X-Forwarded-For`.
pub fn is_trusted_hop(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            ip.is_loopback()
                || ip.is_private()
                || ip.is_link_local()
                || (ip.octets()[0] == 100 && (ip.octets()[1] & 0xc0) == 64)
        }
        IpAddr::V6(ip) => {
            if let Some(v4) = ip.to_ipv4_mapped() {
                return is_trusted_hop(IpAddr::V4(v4));
            }
            ip.is_loopback()
                || (ip.segments()[0] & 0xfe00) == 0xfc00 // unique local fc00::/7
                || (ip.segments()[0] & 0xffc0) == 0xfe80 // link local fe80::/10
        }
    }
}

fn parse_hop(raw: &str) -> Option<IpAddr> {
    let raw = raw.trim();
    let raw = raw.strip_prefix('[').unwrap_or(raw);
    let raw = raw.strip_suffix(']').unwrap_or(raw);
    raw.parse().ok()
}

/// Resolves the client IP from the direct `peer` and, with `trust_proxy`, the
/// `X-Forwarded-For` chain (echo `ExtractIPFromXFFHeader`). Without
/// `trust_proxy` it is the peer (echo `ExtractIPDirect`). `None` only when no
/// peer address is known (requests built in unit tests).
pub fn resolve_client_ip(
    headers: &HeaderMap,
    peer: Option<IpAddr>,
    trust_proxy: bool,
) -> Option<IpAddr> {
    let peer = peer?;
    if !trust_proxy {
        return Some(peer);
    }

    let mut hops: Vec<&str> = headers
        .get_all("x-forwarded-for")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .collect();
    if hops.is_empty() {
        return Some(peer);
    }

    let peer_str = peer.to_string();
    hops.push(&peer_str);
    for hop in hops.iter().rev() {
        let Some(ip) = parse_hop(hop) else {
            // Unable to parse a hop: the whole chain is untrusted.
            return Some(peer);
        };
        if !is_trusted_hop(ip) {
            return Some(ip);
        }
    }
    // Every hop is trusted: best effort, the one furthest from the server.
    hops.first().and_then(|hop| parse_hop(hop)).or(Some(peer))
}

/// Resolves the client IP of a request using the global `HTTP_TRUST_PROXY`
/// setting (off when the config is not initialised).
pub fn client_ip_from_parts(
    headers: &HeaderMap,
    extensions: &axum::http::Extensions,
) -> Option<IpAddr> {
    let peer = extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(addr)| addr.ip());
    let trust_proxy = HttpConfig::try_get().is_some_and(|cfg| cfg.trust_proxy);
    resolve_client_ip(headers, peer, trust_proxy)
}

/// Extractor yielding the resolved client IP as a string (`c.RealIP()`);
/// `None` only when the server runs without connect info (unit tests).
#[derive(Debug, Clone, Default)]
pub struct ClientIp(pub Option<String>);

impl<S> FromRequestParts<S> for ClientIp
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(ClientIp(
            client_ip_from_parts(&parts.headers, &parts.extensions).map(|ip| ip.to_string()),
        ))
    }
}

/// Extractor yielding the client IP and User-Agent that echobackend stores in
/// `sessions` / `auth_activity_logs` / `post_views` (empty values are `None`).
#[derive(Debug, Clone, Default)]
pub struct RequestClient(pub ClientInfo);

impl<S> FromRequestParts<S> for RequestClient
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let ClientIp(ip_address) = ClientIp::from_request_parts(parts, state).await?;
        let user_agent = parts
            .headers
            .get(header::USER_AGENT)
            .and_then(|value| value.to_str().ok())
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        Ok(RequestClient(ClientInfo {
            ip_address: ip_address.filter(|ip| !ip.is_empty()),
            user_agent,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn xff(value: &'static str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static(value));
        headers
    }

    fn ip(raw: &str) -> Option<IpAddr> {
        Some(raw.parse().unwrap())
    }

    #[test]
    fn direct_peer_without_trust_proxy() {
        let headers = xff("203.0.113.195");
        assert_eq!(
            resolve_client_ip(&headers, ip("127.0.0.1"), false),
            ip("127.0.0.1")
        );
    }

    #[test]
    fn walks_xff_from_the_right_behind_trusted_proxy() {
        let headers = xff("203.0.113.195, 70.41.3.18");
        assert_eq!(
            resolve_client_ip(&headers, ip("127.0.0.1"), true),
            ip("70.41.3.18")
        );
        let headers = xff("9.9.9.9, 203.0.113.50, 10.0.0.9");
        assert_eq!(
            resolve_client_ip(&headers, ip("10.0.0.2"), true),
            ip("203.0.113.50")
        );
    }

    #[test]
    fn ignores_xff_from_untrusted_peer() {
        let headers = xff("1.2.3.4");
        assert_eq!(
            resolve_client_ip(&headers, ip("198.51.100.7"), true),
            ip("198.51.100.7")
        );
    }

    #[test]
    fn railway_cgnat_peer_is_trusted() {
        let headers = xff("203.0.113.50");
        for peer in ["100.64.0.3", "100.127.9.9"] {
            assert_eq!(
                resolve_client_ip(&headers, ip(peer), true),
                ip("203.0.113.50")
            );
        }
    }

    #[test]
    fn unparsable_hop_falls_back_to_peer() {
        let headers = xff("203.0.113.50, not-an-ip");
        assert_eq!(
            resolve_client_ip(&headers, ip("10.0.0.2"), true),
            ip("10.0.0.2")
        );
    }

    #[test]
    fn all_trusted_hops_yield_the_leftmost() {
        let headers = xff("10.1.1.1, 192.168.0.4");
        assert_eq!(
            resolve_client_ip(&headers, ip("127.0.0.1"), true),
            ip("10.1.1.1")
        );
    }

    #[test]
    fn no_xff_yields_peer_and_no_peer_yields_none() {
        assert_eq!(
            resolve_client_ip(&HeaderMap::new(), ip("10.0.0.2"), true),
            ip("10.0.0.2")
        );
        assert_eq!(resolve_client_ip(&xff("1.2.3.4"), None, true), None);
    }

    #[test]
    fn bracketed_ipv6_hops_parse() {
        let headers = xff("[2001:db8::1]");
        assert_eq!(
            resolve_client_ip(&headers, ip("::1"), true),
            ip("2001:db8::1")
        );
    }
}
