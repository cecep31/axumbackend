//! Shared heuristics that keep marketing spam out of public feeds.
//!
//! Mirrors echobackend `pkg/antispam`; keep the two in sync when changing
//! thresholds.

/// Tags that mark a post as promotional. Trending excludes these so
/// marketing posts don't crowd out editorial content. Authors should still
/// tag honestly: the report queue treats a missing promo tag on an obvious
/// ad as an aggravating signal, not a loophole.
pub const PROMO_TAGS: [&str; 5] = ["promo", "iklan", "ads", "advertisement", "sponsored"];

/// Caps outbound links in a single post body. Link stuffing is the most
/// common shape of marketing spam; legitimate authors can put extra links in
/// their profile instead.
pub const MAX_EXTERNAL_LINKS_PER_POST: usize = 5;

/// Phrases that almost never appear in legitimate posts but are pervasive in
/// gambling/fraud spam. Single generic words (e.g. "slot") are deliberately
/// NOT listed to avoid false positives like "time slot".
const BANNED_POST_KEYWORDS: [&str; 15] = [
    "togel",
    "gacor",
    "maxwin",
    "judol",
    "pinjol",
    "judi online",
    "judi slot",
    "slot online",
    "scatter hitam",
    "mahjong ways",
    "gates of olympus",
    "pragmatic play",
    "rtp live",
    "rtp slot",
    "pinjaman online",
];

/// Reports whether a tag marks a post as promotional (case-insensitive,
/// surrounding whitespace ignored).
pub fn is_promo_tag(name: &str) -> bool {
    let lower = name.trim().to_lowercase();
    PROMO_TAGS.contains(&lower.as_str())
}

/// Counts outbound links in a post body. Both raw URLs and HTML/markdown
/// links contain a scheme, so counting schemes covers every markup the
/// editor can produce.
pub fn count_external_links(body: &str) -> usize {
    let lower = body.to_lowercase();
    lower.matches("http://").count() + lower.matches("https://").count()
}

/// Returns the first banned phrase found in the title or body, or `None`
/// when the text is clean. Matching is case-insensitive.
pub fn banned_keyword<'a>(title: &'a str, body: &'a str) -> Option<&'static str> {
    let haystack = format!("{title}\n{body}").to_lowercase();
    BANNED_POST_KEYWORDS
        .iter()
        .find(|keyword| haystack.contains(**keyword))
        .copied()
}

/// Returns `None` when a post looks fine, otherwise a short user-facing
/// reason explaining why it was rejected. Callers surface the reason through
/// the 422 envelope so authors learn the rule instead of just hitting a wall.
pub fn reject_reason(title: &str, body: &str) -> Option<String> {
    if let Some(keyword) = banned_keyword(title, body) {
        return Some(format!(
            "contains banned keyword \"{keyword}\". Gambling/fraud spam is not allowed; honest promotions must use the \"promo\" tag and follow the community guidelines"
        ));
    }

    let links = count_external_links(body);
    if links > MAX_EXTERNAL_LINKS_PER_POST {
        return Some(format!(
            "too many external links ({links}, max {MAX_EXTERNAL_LINKS_PER_POST}). Put extra links in your profile or tag the post \"promo\" instead of stuffing links into the body"
        ));
    }

    None
}

/// SQL fragment (`IN (...)`) over [`PROMO_TAGS`] for `LOWER(...)`
/// comparisons. Values are compile-time constants, so interpolation is safe.
pub fn promo_tags_sql_in() -> String {
    PROMO_TAGS
        .iter()
        .map(|tag| format!("'{tag}'"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn promo_tag_matching_is_case_insensitive() {
        for name in [
            "promo",
            "Promo",
            " PROMO ",
            "iklan",
            "ads",
            "advertisement",
            "sponsored",
        ] {
            assert!(is_promo_tag(name), "{name} should be a promo tag");
        }
        for name in ["golang", "tutorial", "promo-code", "", "ad"] {
            assert!(!is_promo_tag(name), "{name} should not be a promo tag");
        }
    }

    #[test]
    fn clean_post_is_accepted() {
        let body = "A genuine tutorial with a [docs link](https://example.com/docs).";
        assert_eq!(reject_reason("How to write Rust", body), None);
    }

    #[test]
    fn time_slot_is_not_gambling() {
        // "slot" alone must not trigger: false positives erode author trust.
        let body = "Pick a time slot for the meeting.";
        assert_eq!(reject_reason("Scheduling", body), None);
    }

    #[test]
    fn link_stuffing_is_rejected_at_limit_plus_one() {
        let over: String = (0..=MAX_EXTERNAL_LINKS_PER_POST)
            .map(|_| "https://example.com/item ")
            .collect();
        assert!(reject_reason("Buy now", &over).is_some());

        let at_limit: String = (0..MAX_EXTERNAL_LINKS_PER_POST)
            .map(|_| "https://example.com/item ")
            .collect();
        assert_eq!(reject_reason("Buy now", &at_limit), None);
    }

    #[test]
    fn banned_keywords_are_rejected() {
        assert!(reject_reason("Promo menarik", "Bonus gacor hari ini").is_some());
        assert!(reject_reason("Situs togel terpercaya 2026", "Konten biasa").is_some());
        assert!(reject_reason("Info", "RTP LIVE 98% di sini").is_some());
        assert!(reject_reason("Info", "Pinjaman online cepat cair").is_some());
    }
}
