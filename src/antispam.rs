//! Shared heuristics that keep marketing spam out of public feeds.
//!
//! Mirrors echobackend `pkg/antispam`; keep the two in sync when changing
//! thresholds.

mod links;
mod normalize;

pub use links::{blocked_link, count_external_links};
use normalize::normalize;

/// Tags that mark a post as promotional. Trending excludes these so
/// marketing posts don't crowd out editorial content. Authors should still
/// tag honestly: the report queue treats a missing promo tag on an obvious
/// ad as an aggravating signal, not a loophole.
pub const PROMO_TAGS: [&str; 5] = ["promo", "iklan", "ads", "advertisement", "sponsored"];

/// Caps outbound links in a single post body. Link stuffing is the most
/// common shape of marketing spam; legitimate authors can put extra links in
/// their profile instead.
pub const MAX_EXTERNAL_LINKS_PER_POST: usize = 5;

/// Phrases that almost never appear in legitimate content but are pervasive
/// in gambling/fraud spam. Single generic words (e.g. "slot") are
/// deliberately NOT listed to avoid false positives like "time slot".
/// Entries must be lowercase ASCII words separated by single spaces, the
/// form [`normalize`] produces.
const BANNED_KEYWORDS: [&str; 15] = [
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

/// Returns the first banned phrase found in any of `texts`, or `None` when
/// they are clean. Texts are normalized first, so case, homoglyphs,
/// letter-spacing and leetspeak don't hide a keyword. Multi-word phrases
/// also match when written as one word ("judionline").
pub fn banned_keyword(texts: &[&str]) -> Option<&'static str> {
    let haystack = normalize(&texts.join("\n"));
    BANNED_KEYWORDS
        .iter()
        .copied()
        .find(|keyword| haystack.contains(keyword) || haystack.contains(&keyword.replace(' ', "")))
}

/// Returns `None` when user-written text (comments, guild names and
/// descriptions) looks fine, otherwise a short user-facing reason.
pub fn text_reject_reason(texts: &[&str]) -> Option<String> {
    if let Some(keyword) = banned_keyword(texts) {
        return Some(format!(
            "contains banned keyword \"{keyword}\". Gambling/fraud spam is not allowed; honest promotions must use the \"promo\" tag and follow the community guidelines"
        ));
    }
    texts.iter().find_map(|text| blocked_link(text))
}

/// Returns `None` when a post looks fine, otherwise a short user-facing
/// reason explaining why it was rejected. Tags go through the same keyword
/// check as the text. Callers surface the reason through the 422 envelope so
/// authors learn the rule instead of just hitting a wall.
pub fn post_reject_reason(title: &str, body: &str, tags: &[String]) -> Option<String> {
    let texts: Vec<&str> = [title, body]
        .into_iter()
        .chain(tags.iter().map(String::as_str))
        .collect();
    if let Some(reason) = text_reject_reason(&texts) {
        return Some(reason);
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
        assert_eq!(post_reject_reason("How to write Rust", body, &[]), None);
    }

    #[test]
    fn time_slot_is_not_gambling() {
        // "slot" alone must not trigger: false positives erode author trust.
        let body = "Pick a time slot for the meeting.";
        assert_eq!(post_reject_reason("Scheduling", body, &[]), None);
    }

    #[test]
    fn link_stuffing_is_rejected_at_limit_plus_one() {
        let over: String = (0..=MAX_EXTERNAL_LINKS_PER_POST)
            .map(|_| "https://example.com/item ")
            .collect();
        assert!(post_reject_reason("Buy now", &over, &[]).is_some());

        let at_limit: String = (0..MAX_EXTERNAL_LINKS_PER_POST)
            .map(|_| "https://example.com/item ")
            .collect();
        assert_eq!(post_reject_reason("Buy now", &at_limit, &[]), None);
    }

    #[test]
    fn banned_keywords_are_rejected() {
        assert!(post_reject_reason("Promo menarik", "Bonus gacor hari ini", &[]).is_some());
        assert!(post_reject_reason("Situs togel terpercaya 2026", "Konten biasa", &[]).is_some());
        assert!(post_reject_reason("Info", "RTP LIVE 98% di sini", &[]).is_some());
        assert!(post_reject_reason("Info", "Pinjaman online cepat cair", &[]).is_some());
    }

    #[test]
    fn banned_tags_are_rejected() {
        let tags = ["golang".to_string(), "Slot Gacor".to_string()];
        assert!(post_reject_reason("Info", "Konten biasa", &tags).is_some());
    }

    #[test]
    fn obfuscated_spellings_are_rejected() {
        for text in [
            "Situs g4c0r hari ini",
            "Main di situs G A C O R sekarang",
            "Bonus t.o.g.e.l setiap hari",
            "Bonus ga\u{200b}cor setiap hari",
            "Bonus g\u{0430}cor setiap hari",
            "Bonus gàcór setiap hari",
            "Bonus ｇａｃｏｒ setiap hari",
            "Bonus 𝐠𝐚𝐜𝐨𝐫 setiap hari",
            "Bonus g@cor setiap hari",
            "Daftar judionline sekarang",
            "Daftar judi\n\nonline sekarang",
            "Daftar di gacor88 sekarang",
        ] {
            assert!(banned_keyword(&[text]).is_some(), "{text:?} should match");
        }
    }

    #[test]
    fn normalization_avoids_false_positives() {
        for text in [
            "Our mega corp acquired a startup.",
            "Helps the team to gel quickly.",
            "Released on 2026-10-18 with 98% coverage.",
            "Pick a time slot, then go online to confirm.",
            "Café crème with a résumé attached.",
        ] {
            assert_eq!(banned_keyword(&[text]), None, "{text:?} should be clean");
        }
    }

    #[test]
    fn blocked_links_are_rejected() {
        for text in [
            "Klik https://bit.ly/abc123",
            "Klik bit.ly/abc123",
            "Klik [di sini](https://www.s.id/promo)",
            "Daftar https://slot88-resmi.xyz/daftar",
            "Daftar http://mega-casino.example",
            "Daftar https://sbobet.example:8080/",
        ] {
            assert!(blocked_link(text).is_some(), "{text:?} should be blocked");
        }
        for text in [
            "Docs at https://go.dev/doc and https://github.com/org/repo",
            "Book at https://timeslot.app/booking",
            "Visit orbit.ly/ for the mission log",
            "Plain text without links",
        ] {
            assert_eq!(blocked_link(text), None, "{text:?} should be allowed");
        }
    }

    #[test]
    fn text_reject_reason_covers_keywords_and_links() {
        assert!(text_reject_reason(&["Mantap, cek bit.ly/xyz ya"]).is_some());
        assert_eq!(text_reject_reason(&["Nice write-up, thanks!"]), None);
    }
}
