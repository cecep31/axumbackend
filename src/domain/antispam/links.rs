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

/// Top-level domains that flag a scheme-less "example.com" as a link.
/// Deliberately short: TLDs that double as common code identifiers (".id",
/// ".io", ".info", ".app", ".link", ".store") would reject posts that merely
/// mention user.id or java.io.
const LINK_TLDS: [&str; 9] = [
    "com", "org", "xyz", "biz", "club", "vip", "co", "bet", "casino",
];

/// Messenger/link-in-bio hosts that spam pastes without a scheme and that the
/// TLD list cannot catch ("t.me/promo").
const BARE_LINK_DOMAINS: [&str; 3] = ["t.me", "wa.me", "linktr.ee"];

/// Reports whether text contains a link in any form authors paste: a URL with
/// a scheme, "www." hosts, mailto: addresses, a bare "example.com" domain, or
/// a scheme-less messenger link such as "t.me/x". Markdown and HTML links
/// always carry one of these, so they are covered.
pub fn contains_link(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("://")
        || lower.contains("mailto:")
        || has_www_host(&lower)
        || has_bare_domain(&lower)
        || BARE_LINK_DOMAINS
            .iter()
            .any(|domain| contains_bare_link(&lower, domain))
}

/// Counts links across texts. Text is split into words at whitespace,
/// brackets and quotes, so markdown, HTML and plain links each count once; a
/// word glued from several URLs counts once per scheme.
pub fn count_links(texts: &[&str]) -> usize {
    texts
        .iter()
        .flat_map(|text| text.split(|c: char| c.is_whitespace() || "()[]<>\"'".contains(c)))
        .filter(|word| contains_link(word))
        .map(|word| word.matches("://").count().max(1))
        .sum()
}

/// Matches "www." at the start of a word, not the tail of one ("awww.").
fn has_www_host(lower: &str) -> bool {
    lower.match_indices("www.").any(|(start, _)| {
        lower[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_ascii_alphanumeric())
    })
}

/// Matches "label.tld" for the TLDs in [`LINK_TLDS`], requiring the TLD to end
/// the word so "example.community" does not match ".co".
fn has_bare_domain(lower: &str) -> bool {
    lower.match_indices('.').any(|(dot, _)| {
        let after_label = lower[..dot]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_ascii_alphanumeric());
        if !after_label {
            return false;
        }
        let rest = &lower[dot + 1..];
        let end = rest
            .find(|c: char| !c.is_ascii_lowercase())
            .unwrap_or(rest.len());
        LINK_TLDS.contains(&&rest[..end]) && !rest[end..].starts_with(|c: char| c.is_ascii_digit())
    })
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
