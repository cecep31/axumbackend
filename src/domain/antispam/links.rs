//! Link checks: shortened links and gambling-looking hosts.
//!
//! Mirrors echobackend `pkg/antispam/links.go`.

/// Link shorteners hide a link's destination, which is exactly what gambling
/// and fraud spam relies on. Authors can always paste the full URL instead.
const LINK_SHORTENERS: [&str; 12] = [
    "adf.ly",
    "bit.ly",
    "cutt.ly",
    "is.gd",
    "ow.ly",
    "rb.gy",
    "rebrand.ly",
    "s.id",
    "shorturl.at",
    "t.ly",
    "tinyurl.com",
    "v.gd",
];

/// Words that only appear in hostnames of gambling sites. "slot" is handled
/// separately because legitimate scheduling sites use it too.
const GAMBLING_HOST_WORDS: [&str; 4] = ["casino", "judi", "poker", "sbobet"];

/// Counts outbound links in a post body. Both raw URLs and HTML/markdown
/// links contain a scheme, so counting schemes covers every markup the
/// editor can produce.
pub fn count_external_links(body: &str) -> usize {
    let lower = body.to_lowercase();
    lower.matches("http://").count() + lower.matches("https://").count()
}

/// Returns a user-facing reason for the first link that points to a link
/// shortener or a gambling-looking host, or `None` when every link is fine.
pub fn blocked_link(text: &str) -> Option<String> {
    let lower = text.to_lowercase();
    for host in link_hosts(&lower) {
        if let Some(shortener) = match_shortener(host) {
            return Some(shortener_reason(shortener));
        }
        if is_gambling_host(host) {
            return Some(format!("links to gambling sites are not allowed ({host})"));
        }
    }
    // Shortened links are often pasted without a scheme ("bit.ly/xyz").
    LINK_SHORTENERS
        .iter()
        .find(|shortener| contains_bare_link(&lower, shortener))
        .map(|shortener| shortener_reason(shortener))
}

fn shortener_reason(shortener: &str) -> String {
    format!(
        "link shortener {shortener} is not allowed; paste the full destination URL so readers can see where it goes"
    )
}

/// Extracts the host of every http(s) URL in already-lowercased text,
/// without a "www." prefix or port.
fn link_hosts(lower: &str) -> Vec<&str> {
    let mut hosts = Vec::new();
    let mut rest = lower;
    while let Some(i) = rest.find("://") {
        let scheme = &rest[..i];
        rest = &rest[i + "://".len()..];
        if !scheme.ends_with("http") && !scheme.ends_with("https") {
            continue;
        }
        let end = rest.find(is_host_terminator).unwrap_or(rest.len());
        let host = rest[..end].strip_prefix("www.").unwrap_or(&rest[..end]);
        if !host.is_empty() {
            hosts.push(host);
        }
    }
    hosts
}

fn is_host_terminator(c: char) -> bool {
    !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-')
}

fn match_shortener(host: &str) -> Option<&'static str> {
    LINK_SHORTENERS.iter().copied().find(|shortener| {
        host == *shortener
            || host
                .strip_suffix(shortener)
                .is_some_and(|prefix| prefix.ends_with('.'))
    })
}

fn is_gambling_host(host: &str) -> bool {
    GAMBLING_HOST_WORDS.iter().any(|word| host.contains(word)) || has_slot_number(host)
}

/// Matches the "slot88" / "slot777" naming pattern of gambling sites while
/// leaving hosts like "timeslot.app" alone.
fn has_slot_number(host: &str) -> bool {
    host.match_indices("slot").any(|(i, _)| {
        host.as_bytes()
            .get(i + "slot".len())
            .is_some_and(u8::is_ascii_digit)
    })
}

/// Reports whether `domain` appears as the start of a link path
/// ("bit.ly/abc"), not as the tail of a longer word ("orbit.ly/").
fn contains_bare_link(lower: &str, domain: &str) -> bool {
    let needle = format!("{domain}/");
    lower.match_indices(&needle).any(|(start, _)| {
        lower[..start]
            .chars()
            .next_back()
            .is_none_or(is_host_terminator)
    })
}
