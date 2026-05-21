use base64::{Engine as _, engine::general_purpose::STANDARD};

use crate::model::{BodyPreview, HeaderPair};

const DEFAULT_REQUEST_LIMIT: usize = 1024 * 1024;
const DEFAULT_RESPONSE_LIMIT: usize = 2 * 1024 * 1024;

pub fn redact_header(name: &str, value: &str) -> String {
    let lower = name.to_ascii_lowercase();
    match lower.as_str() {
        "authorization" | "cookie" | "set-cookie" | "proxy-authorization" => "<redacted>".into(),
        _ if lower.contains("api-key") || lower.contains("token") => "<redacted>".into(),
        _ => value.to_string(),
    }
}

pub fn redact_headers(headers: &[HeaderPair]) -> Vec<HeaderPair> {
    headers
        .iter()
        .map(|h| HeaderPair {
            name: h.name.clone(),
            value: redact_header(&h.name, &h.value),
        })
        .collect()
}

pub fn preview_body(content_type: Option<&str>, bytes: &[u8], max_bytes: usize) -> BodyPreview {
    let truncated = bytes.len() > max_bytes;
    let visible = if truncated {
        &bytes[..max_bytes]
    } else {
        bytes
    };
    let content_type = content_type.map(|s| s.to_string());
    let text = if is_textual(content_type.as_deref(), visible) {
        String::from_utf8(visible.to_vec()).ok()
    } else {
        None
    };
    let base64 = if text.is_none() && !visible.is_empty() {
        Some(STANDARD.encode(visible))
    } else {
        None
    };
    BodyPreview {
        content_type,
        truncated,
        size: bytes.len() as u64,
        text,
        base64,
    }
}

pub fn preview_request_body(content_type: Option<&str>, bytes: &[u8]) -> BodyPreview {
    preview_body(content_type, bytes, DEFAULT_REQUEST_LIMIT)
}

pub fn preview_response_body(content_type: Option<&str>, bytes: &[u8]) -> BodyPreview {
    preview_body(content_type, bytes, DEFAULT_RESPONSE_LIMIT)
}

fn is_textual(content_type: Option<&str>, bytes: &[u8]) -> bool {
    if let Some(content_type) = content_type {
        let lower = content_type.to_ascii_lowercase();
        lower.starts_with("text/")
            || lower.contains("json")
            || lower.contains("xml")
            || lower.contains("x-www-form-urlencoded")
    } else {
        std::str::from_utf8(bytes).is_ok()
    }
}
