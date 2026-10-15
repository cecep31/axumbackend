//! Guards shared by multipart upload handlers.
//!
//! Background: axum's `Multipart` extractor streams through the route's
//! `DefaultBodyLimit`. When a body exceeds that cap the stream errors
//! mid-field and multer reports a generic "Error parsing multipart/form-data
//! request", hiding the real cause (almost always: file bigger than the cap).
//! These helpers reject oversized uploads up front with a clear 413, and size
//! route body limits so legitimate files always stream fully.

use crate::error::AppError;

/// Framing overhead (boundaries + part headers) tolerated on top of a file
/// cap when sizing route body limits and pre-checking `Content-Length`.
/// Real overhead is a few hundred bytes; the slack keeps files exactly at
/// the cap safely streamable.
pub const MULTIPART_OVERHEAD_SLACK_BYTES: u64 = 64 * 1024;

/// Total request-body allowance for a single file upload capped at
/// `max_file_bytes`.
pub fn max_body_bytes(max_file_bytes: u64) -> usize {
    (max_file_bytes + MULTIPART_OVERHEAD_SLACK_BYTES) as usize
}

/// Returns true when `Content-Length` already exceeds the allowance.
/// Missing or unparseable lengths return false — the streaming file-size
/// check downstream remains the safety net for those.
pub fn content_length_exceeds(headers: &axum::http::HeaderMap, max_body: u64) -> bool {
    headers
        .get(axum::http::header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .is_some_and(|len| len > max_body)
}

/// 413 guard for upload handlers: call before touching the multipart stream
/// so oversized uploads fail fast with a clear message instead of dying
/// mid-stream with a generic parse error.
pub fn reject_oversized_body(
    headers: &axum::http::HeaderMap,
    max_file_bytes: u64,
    label: &str,
) -> Result<(), AppError> {
    if content_length_exceeds(headers, max_file_bytes + MULTIPART_OVERHEAD_SLACK_BYTES) {
        return Err(AppError::PayloadTooLarge(format!(
            "{label} is too large (max {} MB)",
            max_file_bytes / (1024 * 1024)
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers_with_length(value: &str) -> axum::http::HeaderMap {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(axum::http::header::CONTENT_LENGTH, value.parse().unwrap());
        headers
    }

    #[test]
    fn max_body_bytes_adds_slack() {
        assert_eq!(max_body_bytes(1024 * 1024), 1024 * 1024 + 64 * 1024);
    }

    #[test]
    fn content_length_check() {
        let max = 1024 * 1024 + 64 * 1024;
        assert!(content_length_exceeds(&headers_with_length("9999999"), max));
        assert!(!content_length_exceeds(&headers_with_length("1000"), max));
        assert!(!content_length_exceeds(
            &headers_with_length("not-a-number"),
            max
        ));
        assert!(!content_length_exceeds(&axum::http::HeaderMap::new(), max));
    }

    #[test]
    fn reject_oversized_body_reports_413() {
        let err = reject_oversized_body(&headers_with_length("9999999"), 1024 * 1024, "Image")
            .expect_err("should reject");
        assert!(matches!(err, AppError::PayloadTooLarge(_)));

        assert!(reject_oversized_body(&headers_with_length("1000"), 1024 * 1024, "Image").is_ok());
    }
}
