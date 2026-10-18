//! Text normalization that lets obfuscated spam match the plain keyword list.
//!
//! Mirrors echobackend `pkg/antispam/normalize.go`.

use std::collections::HashMap;
use std::sync::LazyLock;

/// Look-alike characters spammers use to dodge keyword matching, grouped by
/// the ASCII letter they imitate: Latin diacritics, Cyrillic/Greek homoglyphs
/// and the IPA script g. Uppercase forms are covered because input is
/// lowercased first.
const CONFUSABLE_GROUPS: [(char, &str); 20] = [
    ('a', "àáâãäåāăąаα"),
    ('b', "ьв"),
    ('c', "çćĉċčс"),
    ('e', "èéêëēĕėęěеε"),
    ('g', "ĝğġģɡ"),
    ('h', "н"),
    ('i', "ìíîïĩīĭįıіι"),
    ('j', "ј"),
    ('k', "кκ"),
    ('m', "м"),
    ('n', "ñńņň"),
    ('o', "òóôõöøōŏőоο"),
    ('p', "рρ"),
    ('s', "śŝşšѕ"),
    ('t', "тτ"),
    ('u', "ùúûüũūŭůűųυ"),
    ('v', "ν"),
    ('x', "х"),
    ('y', "ýÿŷу"),
    ('z', "źżž"),
];

static CONFUSABLES: LazyLock<HashMap<char, char>> = LazyLock::new(|| {
    CONFUSABLE_GROUPS
        .iter()
        .flat_map(|&(ascii, lookalikes)| lookalikes.chars().map(move |c| (c, ascii)))
        .collect()
});

/// Reduces text to lowercase ASCII words separated by single spaces:
///
/// - fullwidth, math-styled and circled letters fold to ASCII ("𝐠𝐚𝐜𝐨𝐫");
/// - homoglyphs and diacritics fold to ASCII ("gаcor" with Cyrillic а);
/// - invisible characters are dropped ("ga\u{200b}cor");
/// - runs of single characters merge ("g a c o r", "g.a.c.o.r");
/// - leetspeak digits are undone inside words ("g4c0r").
pub(super) fn normalize(text: &str) -> String {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for c in text.to_lowercase().chars() {
        if is_invisible(c) {
            continue;
        }
        let folded = fold_char(c);
        if folded.is_ascii_lowercase() || folded.is_ascii_digit() {
            current.push(folded);
        } else if !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }

    merge_single_char_runs(tokens)
        .iter()
        .map(|token| undo_leet(token))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Maps a character to its plain ASCII look-alike, or returns it unchanged.
/// '@' and '$' fold too because they stand in for letters ("g@cor", "$lot").
fn fold_char(c: char) -> char {
    let code = u32::from(c);
    match code {
        0x40 => 'a',
        0x24 => 's',
        // Fullwidth digits and letters.
        0xFF10..=0xFF5A => char::from_u32(code - 0xFEE0).map_or(c, |a| a.to_ascii_lowercase()),
        // Mathematical alphanumeric letters.
        0x1D400..=0x1D6A3 => ascii_offset(b'a', (code - 0x1D400) % 52 % 26),
        // Mathematical digits.
        0x1D7CE..=0x1D7FF => ascii_offset(b'0', (code - 0x1D7CE) % 10),
        // Circled letters.
        0x24B6..=0x24E9 => ascii_offset(b'a', (code - 0x24B6) % 26),
        _ => CONFUSABLES.get(&c).copied().unwrap_or(c),
    }
}

fn ascii_offset(base: u8, offset: u32) -> char {
    // Callers reduce `offset` modulo 26 or 10, so it always fits in a u8.
    char::from(base + offset as u8)
}

/// Characters that render as nothing (soft hyphen, zero-width and bidi
/// controls, BOM) or only decorate the previous letter (combining marks).
/// Dropping them glues the surrounding letters back together instead of
/// splitting the word.
fn is_invisible(c: char) -> bool {
    matches!(
        u32::from(c),
        0x00AD | 0xFEFF | 0x0300..=0x036F | 0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x2064
    )
}

/// Joins consecutive one-character tokens, undoing letter-spacing
/// ("g a c o r" -> "gacor") without gluing real words together ("mega corp"
/// stays two tokens).
fn merge_single_char_runs(tokens: Vec<String>) -> Vec<String> {
    let mut merged = Vec::with_capacity(tokens.len());
    let mut run = String::new();
    for token in tokens {
        if token.len() == 1 {
            run.push_str(&token);
            continue;
        }
        if !run.is_empty() {
            merged.push(std::mem::take(&mut run));
        }
        merged.push(token);
    }
    if !run.is_empty() {
        merged.push(run);
    }
    merged
}

/// Undoes digit-for-letter swaps ("g4c0r" -> "gacor"). Only tokens that also
/// contain a letter are touched, so plain numbers such as "2026" stay as-is.
fn undo_leet(token: &str) -> String {
    if !token.bytes().any(|b| b.is_ascii_lowercase()) {
        return token.to_string();
    }
    token
        .chars()
        .map(|c| match c {
            '0' => 'o',
            '1' => 'i',
            '3' => 'e',
            '4' => 'a',
            '5' => 's',
            '7' => 't',
            other => other,
        })
        .collect()
}
