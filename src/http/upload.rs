//! Guards shared by multipart upload handlers.
//!
//! Background: axum's `Multipart` extractor streams through the route's
//! `DefaultBodyLimit`. When a body exceeds that cap the stream errors
//! mid-field and multer reports a generic "Error parsing multipart/form-data
//! request", hiding the real cause (almost always: file bigger than the cap).
//! These helpers reject oversized uploads up front with a clear 413, and size
//! route body limits so legitimate files always stream fully.
//!
//! It also holds the shared single-image upload flow (multipart read loop,
//! magic-byte sniffing, storage save) used by the avatar and post image
//! endpoints; each endpoint parametrizes it with an [`ImageUploadSpec`].

use crate::http::error::AppError;
use axum::extract::Multipart;

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

/// Per-endpoint knobs of the single-image upload flow.
#[derive(Debug, Clone, Copy)]
pub struct ImageUploadSpec {
    /// Multipart field carrying the file; other fields are skipped.
    pub field: &'static str,
    /// Maximum file size in bytes (the error message reports it in whole MB).
    pub max_file_bytes: u64,
    /// Capitalized noun used in the 413 messages ("Avatar", "Image").
    pub label: &'static str,
    /// Object key prefix the file is stored under.
    pub key_prefix: &'static str,
    /// Prefix of log lines ("avatar", "post image").
    pub log_name: &'static str,
}

impl ImageUploadSpec {
    /// 413 for a file over the cap: "<label> is too large (max N MB)".
    fn too_large(&self) -> AppError {
        AppError::PayloadTooLarge(format!(
            "{} is too large (max {} MB)",
            self.label,
            self.max_file_bytes / (1024 * 1024)
        ))
    }
}

/// An uploaded file that passed the size cap and the image sniffing.
#[derive(Debug)]
pub struct SniffedImage {
    pub data: axum::body::Bytes,
    pub content_type: &'static str,
    pub ext: &'static str,
}

/// Returns `(content_type, extension)` for allowed images: JPEG, PNG, GIF
/// and WEBP, judged by magic bytes like echobackend's `detectAllowedImage`
/// (the client's declared content type and file name are ignored).
pub fn detect_allowed_image(data: &[u8]) -> Option<(&'static str, &'static str)> {
    if data.len() >= 6 && (&data[0..6] == b"GIF87a" || &data[0..6] == b"GIF89a") {
        Some(("image/gif", ".gif"))
    } else if data.len() >= 3 && data[0..3] == [0xff, 0xd8, 0xff] {
        Some(("image/jpeg", ".jpg"))
    } else if data.len() >= 8 && &data[0..8] == b"\x89PNG\r\n\x1a\n" {
        Some(("image/png", ".png"))
    } else if data.len() >= 12 && &data[0..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        Some(("image/webp", ".webp"))
    } else {
        None
    }
}

/// Reads the first `spec.field` part of the multipart stream and checks its
/// size and type. Errors: 400 "Failed to upload image: .." / "Failed to read
/// image: .." on a broken stream, 413 "<label> is too large (max N MB)",
/// 400 "Invalid file type", 400 "No file uploaded" when the field is absent.
pub async fn read_image_field(
    multipart: &mut Multipart,
    spec: &ImageUploadSpec,
) -> Result<SniffedImage, AppError> {
    while let Some(field) = multipart.next_field().await.map_err(|err| {
        tracing::warn!(error = ?err, "{}: multipart field error", spec.log_name);
        AppError::BadRequest(format!("Failed to upload image: {err}"))
    })? {
        if field.name() != Some(spec.field) {
            continue;
        }

        let data = field.bytes().await.map_err(|err| {
            tracing::warn!(error = ?err, "{}: multipart read error", spec.log_name);
            AppError::BadRequest(format!("Failed to read image: {err}"))
        })?;
        if data.len() as u64 > spec.max_file_bytes {
            return Err(spec.too_large());
        }
        let Some((content_type, ext)) = detect_allowed_image(&data) else {
            return Err(AppError::BadRequest("Invalid file type".to_string()));
        };
        return Ok(SniffedImage {
            data,
            content_type,
            ext,
        });
    }

    Err(AppError::BadRequest("No file uploaded".to_string()))
}

/// Saves the image under a random key below `spec.key_prefix` and returns
/// the key. 400 "Storage is not configured" without storage, 500 "Failed to
/// upload image" when the save fails.
pub async fn store_image(image: SniffedImage, spec: &ImageUploadSpec) -> Result<String, AppError> {
    let Some(storage) = crate::infra::storage::get() else {
        return Err(AppError::BadRequest(
            "Storage is not configured".to_string(),
        ));
    };

    let key = crate::infra::storage::random_object_key(spec.key_prefix, image.ext);
    storage
        .save(&key, image.data.to_vec(), image.content_type)
        .await
        .map_err(|err| {
            tracing::error!(error = %err, key = %key, "{}: upload failed", spec.log_name);
            AppError::InternalServerError("Failed to upload image".to_string())
        })?;
    Ok(key)
}

/// Full single-image upload: 413 pre-check on `Content-Length`, read and
/// sniff `spec.field`, then store it. Returns the stored object key.
pub async fn upload_image(
    headers: &axum::http::HeaderMap,
    multipart: &mut Multipart,
    spec: &ImageUploadSpec,
) -> Result<String, AppError> {
    // Fail fast on oversized bodies: without this the route's body limit
    // kills the stream mid-field and multer reports a generic "Error parsing
    // multipart/form-data request" instead of a clear 413.
    reject_oversized_body(headers, spec.max_file_bytes, spec.label)?;
    let image = read_image_field(multipart, spec).await?;
    store_image(image, spec).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::FromRequest;

    const SPEC: ImageUploadSpec = ImageUploadSpec {
        field: "image",
        max_file_bytes: 1024 * 1024,
        label: "Image",
        key_prefix: "posts/images",
        log_name: "test image",
    };

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00";

    async fn multipart_with(parts: &[(&str, &[u8])]) -> Multipart {
        let boundary = "XBOUNDARYX";
        let mut body = Vec::new();
        for (name, data) in parts {
            body.extend_from_slice(
                format!(
                    "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"f\"\r\nContent-Type: application/octet-stream\r\n\r\n"
                )
                .as_bytes(),
            );
            body.extend_from_slice(data);
            body.extend_from_slice(b"\r\n");
        }
        body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
        let request = axum::http::Request::builder()
            .method("POST")
            .header(
                axum::http::header::CONTENT_TYPE,
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(axum::body::Body::from(body))
            .unwrap();
        Multipart::from_request(request, &()).await.unwrap()
    }

    fn bad_request_message(err: AppError) -> String {
        match err {
            AppError::BadRequest(msg) => msg,
            other => panic!("expected BadRequest, got {other:?}"),
        }
    }

    #[test]
    fn detect_allowed_image_sniffs_magic_bytes() {
        assert_eq!(
            detect_allowed_image(b"GIF89a\x01\x00"),
            Some(("image/gif", ".gif"))
        );
        assert_eq!(
            detect_allowed_image(b"GIF87a\x01\x00"),
            Some(("image/gif", ".gif"))
        );
        assert_eq!(
            detect_allowed_image(&[0xff, 0xd8, 0xff, 0xe0]),
            Some(("image/jpeg", ".jpg"))
        );
        assert_eq!(detect_allowed_image(PNG), Some(("image/png", ".png")));
        assert_eq!(
            detect_allowed_image(b"RIFF\x00\x00\x00\x00WEBPVP8 "),
            Some(("image/webp", ".webp"))
        );
    }

    #[test]
    fn detect_allowed_image_rejects_others() {
        assert_eq!(detect_allowed_image(b""), None);
        assert_eq!(detect_allowed_image(b"GIF8"), None);
        assert_eq!(detect_allowed_image(&[0xff, 0xd8]), None);
        assert_eq!(detect_allowed_image(b"\x89PNG\r\n"), None);
        assert_eq!(detect_allowed_image(b"RIFF\x00\x00\x00\x00WAVE"), None);
        assert_eq!(detect_allowed_image(b"plain text"), None);
        assert_eq!(detect_allowed_image(b"%PDF-1.7"), None);
    }

    #[test]
    fn too_large_message_per_endpoint() {
        let avatar = ImageUploadSpec {
            field: "avatar",
            max_file_bytes: 5 * 1024 * 1024,
            label: "Avatar",
            ..SPEC
        };
        for (spec, expected) in [
            (avatar, "Avatar is too large (max 5 MB)"),
            (SPEC, "Image is too large (max 1 MB)"),
        ] {
            match spec.too_large() {
                AppError::PayloadTooLarge(msg) => assert_eq!(msg, expected),
                other => panic!("expected PayloadTooLarge, got {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn read_image_field_picks_named_field() {
        let mut multipart = multipart_with(&[("other", b"GIF89a"), ("image", PNG)]).await;
        let image = read_image_field(&mut multipart, &SPEC).await.unwrap();
        assert_eq!(image.content_type, "image/png");
        assert_eq!(image.ext, ".png");
        assert_eq!(&image.data[..], PNG);
    }

    #[tokio::test]
    async fn read_image_field_errors() {
        let mut multipart = multipart_with(&[("avatar", PNG)]).await;
        let err = read_image_field(&mut multipart, &SPEC).await.unwrap_err();
        assert_eq!(bad_request_message(err), "No file uploaded");

        let mut multipart = multipart_with(&[("image", b"plain text")]).await;
        let err = read_image_field(&mut multipart, &SPEC).await.unwrap_err();
        assert_eq!(bad_request_message(err), "Invalid file type");

        let big = vec![0xffu8; (SPEC.max_file_bytes + 1) as usize];
        let mut multipart = multipart_with(&[("image", &big)]).await;
        let err = read_image_field(&mut multipart, &SPEC).await.unwrap_err();
        match err {
            AppError::PayloadTooLarge(msg) => assert_eq!(msg, "Image is too large (max 1 MB)"),
            other => panic!("expected PayloadTooLarge, got {other:?}"),
        }
    }

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
