//! Turns human names into URL-safe identifiers (echobackend's `pkg/slug`).

/// Converts `s` into a lowercase, hyphen-separated slug: runs of anything that
/// is not a letter or digit collapse into a single hyphen, and leading and
/// trailing hyphens are trimmed. Non-ASCII letters are kept as-is, so a name
/// that is entirely non-Latin still yields a usable slug instead of an empty
/// string. The result is truncated to `max_len` characters (0 means no limit).
pub fn make(s: &str, max_len: usize) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_hyphen = false;
    for c in s.trim().to_lowercase().chars() {
        if c.is_alphabetic() || c.is_numeric() {
            out.push(c);
            prev_hyphen = false;
        } else if !prev_hyphen && !out.is_empty() {
            out.push('-');
            prev_hyphen = true;
        }
    }

    let mut out = out.trim_matches('-').to_string();
    if max_len > 0
        && let Some((idx, _)) = out.char_indices().nth(max_len)
    {
        out.truncate(idx);
    }
    out.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_make() {
        let cases = [
            ("Guild Dokter Indonesia", 100, "guild-dokter-indonesia"),
            ("Go & Rust  Devs!!", 100, "go-rust-devs"),
            ("  --Hello--  ", 100, "hello"),
            ("Angkatan 2026", 100, "angkatan-2026"),
            ("my_guild_name", 100, "my-guild-name"),
            ("abcdef ghij", 7, "abcdef"),
            ("Комната", 100, "комната"),
            ("!!!???", 100, ""),
            ("", 100, ""),
            ("a b c", 0, "a-b-c"),
        ];
        for (input, limit, want) in cases {
            assert_eq!(make(input, limit), want, "make({input:?}, {limit})");
        }
    }
}
