const DEFAULT_CONVERSATION_TITLE: &str = "New conversation";
/// Judul otomatis dari pesan pertama: pendek ala daftar chat.
const MAX_TITLE_WORDS: usize = 8;
const MAX_TITLE_CHARS: usize = 60;
/// Judul eksplisit: ikuti kapasitas kolom DB (varchar 255).
const MAX_EXPLICIT_TITLE_CHARS: usize = 255;

pub(super) fn build_conversation_title(title: Option<String>, content: &str) -> String {
    if let Some(title) = title {
        let trimmed = title.trim();
        if !trimmed.is_empty() {
            // Judul eksplisit dari client: hormati isinya, cukup rapikan
            // whitespace dan batasi ke kapasitas kolom DB.
            return truncate_chars(&normalize_space(trimmed), MAX_EXPLICIT_TITLE_CHARS);
        }
    }
    let normalized = normalize_space(content);
    if normalized.is_empty() {
        return DEFAULT_CONVERSATION_TITLE.to_string();
    }
    // Ambil baris/kalimat pertama agar tidak kepotong tengah kalimat.
    let candidate = first_sentence(&normalized);
    let mut words: Vec<&str> = candidate.split_whitespace().collect();
    words = strip_leading_fillers(words);
    words = strip_trailing_fillers(words);
    if words.is_empty() {
        words = candidate.split_whitespace().collect();
    }
    let title = truncate_words(&words, MAX_TITLE_WORDS, MAX_TITLE_CHARS);
    if title.is_empty() {
        return DEFAULT_CONVERSATION_TITLE.to_string();
    }
    title
}

/// Memadatkan semua whitespace menjadi satu spasi.
fn normalize_space(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Memotong string per char (aman untuk UTF-8).
fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    s.chars().take(max).collect()
}

const TITLE_TRIM_CHARS: &[char] = &[
    ' ', '\t', '"', '\'', '\u{201c}', '\u{201d}', '\u{2018}', '\u{2019}', '.', ',', ':', ';', '!',
    '?', '\u{2026}', '-', '\u{2013}', '\u{2014}',
];

/// Mengambil baris pertama, lalu potong di akhir kalimat pertama
/// (. ! ? …) bila prefix-nya sudah cukup bermakna (>=3 kata).
/// Juga membersihkan prefix markdown/list seperti "# ", "> ", "- ", "1. ".
fn first_sentence(s: &str) -> String {
    let mut s = s.split('\n').next().unwrap_or("").trim();
    if s.is_empty() {
        return String::new();
    }
    s = s.trim_start_matches([
        '#', '>', '*', '-', '\u{2022}', '\u{2013}', '\u{2014}', ' ', '\t',
    ]);
    if let Some(dot) = s.find(". ") {
        let head = s[..dot].trim();
        if head.split_whitespace().count() >= 3 {
            s = head;
        }
    }
    let mut end: Option<usize> = None;
    for (i, c) in s.char_indices() {
        if c == '!' || c == '?' || c == '\u{2026}' || c == '.' {
            end = Some(i + c.len_utf8());
            break;
        }
    }
    if let Some(end) = end {
        let head = s[..end].trim();
        if head.split_whitespace().count() >= 3 {
            s = head;
        }
    }
    // Strip "1. ", "12) " ala numbered list di awal.
    let digits_len = s
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .map(char::len_utf8)
        .sum::<usize>();
    if digits_len > 0 && digits_len < s.len() {
        let rest = &s[digits_len..];
        if let Some(c) = rest.chars().next()
            && (c == '.' || c == ')')
        {
            let after = rest[c.len_utf8()..].trim();
            if !after.is_empty() {
                s = after;
            }
        }
    }
    s.trim_matches(TITLE_TRIM_CHARS).to_string()
}

/// Kata pengisi di awal pesan (ID/EN) yang buruk untuk judul.
fn is_leading_filler(word: &str) -> bool {
    matches!(
        word,
        "tolong"
            | "mohon"
            | "please"
            | "pls"
            | "plis"
            | "coba"
            | "cobalah"
            | "bisakah"
            | "bisa"
            | "bolehkah"
            | "gimana"
            | "bagaimana"
            | "cara"
            | "buatkan"
            | "buatin"
            | "bikinkan"
            | "bikinin"
            | "tuliskan"
            | "tulis"
            | "tuliskanlah"
            | "jelaskan"
            | "jelasin"
            | "jelasken"
            | "kasih"
            | "kasi"
            | "berikan"
            | "beritahu"
            | "beritahukan"
            | "tunjukkan"
            | "tunjukin"
            | "apa"
            | "apakah"
            | "itu"
            | "ini"
            | "halo"
            | "haloo"
            | "hallo"
            | "hai"
            | "hi"
            | "hello"
    )
}

/// Kata pengisi di akhir pesan yang buruk untuk judul.
fn is_trailing_filler(word: &str) -> bool {
    matches!(
        word,
        "ya" | "yah" | "dong" | "donk" | "sih" | "deh" | "loh" | "lho" | "kah" | "tuh"
    )
}

fn strip_leading_fillers(mut words: Vec<&str>) -> Vec<&str> {
    let mut stripped = 0;
    while stripped < 2 {
        let Some(first) = words.first() else {
            break;
        };
        if !is_leading_filler(&first.to_lowercase()) {
            break;
        }
        words.remove(0);
        stripped += 1;
    }
    words
}

fn strip_trailing_fillers(mut words: Vec<&str>) -> Vec<&str> {
    while words.len() > 3 {
        let Some(last) = words.last() else {
            break;
        };
        if !is_trailing_filler(&last.to_lowercase()) {
            break;
        }
        words.pop();
    }
    words
}

/// Menggabung max_words kata pertama dan memastikan panjang tidak melebihi
/// max_chars, selalu potong di batas kata (tidak pernah memotong tengah
/// kata maupun tengah char UTF-8).
fn truncate_words(words: &[&str], max_words: usize, max_chars: usize) -> String {
    let head: Vec<&&str> = words.iter().take(max_words).collect();
    let joined: String = head
        .into_iter()
        .map(|s| s.as_ref())
        .collect::<Vec<_>>()
        .join(" ");
    let out = joined.trim_matches(TITLE_TRIM_CHARS).to_string();
    if out.is_empty() {
        return String::new();
    }
    if out.chars().count() <= max_chars {
        return out;
    }
    let cutoff: String = out.chars().take(max_chars).collect();
    match cutoff.rfind(' ') {
        Some(0) | None => cutoff,
        Some(i) => cutoff[..i]
            .trim_end_matches([
                ' ', '\t', '.', ',', ':', ';', '!', '?', '\u{2026}', '-', '\u{2013}', '\u{2014}',
            ])
            .to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn title(content: &str) -> String {
        build_conversation_title(None, content)
    }

    #[test]
    fn explicit_title_is_respected() {
        let got = build_conversation_title(Some("  Laporan Q1  ".to_string()), "abaikan ini");
        assert_eq!(got, "Laporan Q1");
    }

    #[test]
    fn empty_content_falls_back() {
        assert_eq!(title("   "), "New conversation");
    }

    #[test]
    fn strips_leading_filler_words() {
        assert_eq!(
            title("tolong buatkan laporan keuangan bulan januari untuk presentasi besok"),
            "laporan keuangan bulan januari untuk presentasi besok"
        );
    }

    #[test]
    fn takes_first_sentence() {
        assert_eq!(
            title("Apa itu inflasi? Jelaskan dampaknya ke pasar saham Indonesia secara detail"),
            "inflasi"
        );
    }

    #[test]
    fn truncates_at_word_boundary() {
        assert_eq!(
            title(
                "analisis perbandingan saham bank BCA BRI Mandiri BNI BTN CIMB Danamon Permata OCBC tahun ini"
            ),
            "analisis perbandingan saham bank BCA BRI Mandiri BNI"
        );
    }

    #[test]
    fn never_splits_utf8_char() {
        let got = title(
            "ceritakan tentang café crème brûlée naïve façade di kota Zürich yang indah sekali dan menawan hati",
        );
        assert_eq!(got, "ceritakan tentang café crème brûlée naïve façade di");
        assert!(got.chars().count() <= MAX_TITLE_CHARS);
    }

    #[test]
    fn strips_markdown_and_list_prefix() {
        assert_eq!(
            title("### 1. Jelaskan strategi investasi untuk pemula yang baru mulai"),
            "strategi investasi untuk pemula yang baru mulai"
        );
    }

    #[test]
    fn strips_trailing_filler() {
        assert_eq!(
            title("jelaskan bedanya saham dan obligasi untuk pemula ya"),
            "bedanya saham dan obligasi untuk pemula"
        );
    }

    #[test]
    fn explicit_long_title_truncated_to_db_limit() {
        let long = "a".repeat(300);
        let got = build_conversation_title(Some(long), "fallback");
        assert_eq!(got.chars().count(), MAX_EXPLICIT_TITLE_CHARS);
    }
}
