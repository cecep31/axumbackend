use crate::dto::validation::required;
use garde::Validate;
use serde::Deserialize;

/// echobackend `CreateTagRequest`: `name` `required,min=1,max=30`.
#[derive(Deserialize, Validate, Default)]
#[serde(default)]
pub struct CreateTagRequest {
    #[garde(custom(required), length(chars, min = 1, max = 30))]
    pub name: String,
}

/// echobackend `UpdateTagRequest`: `name` `required,min=1,max=30`.
#[derive(Deserialize, Validate, Default)]
#[serde(default)]
pub struct UpdateTagRequest {
    #[garde(custom(required), length(chars, min = 1, max = 30))]
    pub name: String,
}

/// echobackend `strconv.ParseUint(c.Param("id"), 10, 32)`: an unsigned
/// base-10 integer in `0..=u32::MAX` (no sign, no whitespace). On failure the
/// error is Go's `*strconv.NumError` text.
pub fn parse_tag_id(raw: &str) -> Result<u32, String> {
    let quoted = format!("{raw:?}");
    if raw.is_empty() || !raw.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!(
            "strconv.ParseUint: parsing {quoted}: invalid syntax"
        ));
    }
    raw.parse::<u32>()
        .map_err(|_| format!("strconv.ParseUint: parsing {quoted}: value out of range"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::validation::failing_tags;

    #[test]
    fn tag_name_is_required() {
        let missing: CreateTagRequest = serde_json::from_str("{}").unwrap();
        assert_eq!(
            failing_tags(&missing),
            vec![("Name".to_string(), "required".to_string())]
        );
        let empty: UpdateTagRequest = serde_json::from_str(r#"{"name":""}"#).unwrap();
        assert_eq!(
            failing_tags(&empty),
            vec![("Name".to_string(), "required".to_string())]
        );
        let long = CreateTagRequest {
            name: "x".repeat(31),
        };
        assert_eq!(
            failing_tags(&long),
            vec![("Name".to_string(), "max".to_string())]
        );
    }

    #[test]
    fn tag_id_parses_like_go_parse_uint32() {
        assert_eq!(parse_tag_id("0"), Ok(0));
        assert_eq!(parse_tag_id("42"), Ok(42));
        assert_eq!(parse_tag_id("007"), Ok(7));
        assert_eq!(parse_tag_id("4294967295"), Ok(u32::MAX));
        assert_eq!(
            parse_tag_id("4294967296").unwrap_err(),
            r#"strconv.ParseUint: parsing "4294967296": value out of range"#
        );
        for bad in ["", "abc", "-1", "+1", " 1", "1.5"] {
            assert_eq!(
                parse_tag_id(bad).unwrap_err(),
                format!("strconv.ParseUint: parsing {bad:?}: invalid syntax")
            );
        }
    }
}
