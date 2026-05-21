use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::Value;

use crate::model::{BodyPreview, HeaderPair, WebSocketFramePreview};

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
        .map(|header| HeaderPair {
            name: header.name.clone(),
            value: redact_header(&header.name, &header.value),
        })
        .collect()
}

pub fn preview_body(content_type: Option<&str>, bytes: &[u8], max_bytes: usize) -> BodyPreview {
    preview_body_with_encoding(content_type, None, bytes, max_bytes)
}

pub fn preview_body_with_encoding(
    content_type: Option<&str>,
    content_encoding: Option<&str>,
    bytes: &[u8],
    max_bytes: usize,
) -> BodyPreview {
    let truncated = bytes.len() > max_bytes;
    let visible = if truncated {
        &bytes[..max_bytes]
    } else {
        bytes
    };
    let content_type = content_type.map(|s| s.to_string());
    let encoding = content_encoding
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let text = if is_textual(content_type.as_deref(), visible) {
        String::from_utf8(visible.to_vec()).ok()
    } else {
        None
    };
    let pretty = pretty_body(content_type.as_deref(), text.as_deref(), visible);
    let base64 = if text.is_none() && !visible.is_empty() {
        Some(STANDARD.encode(visible))
    } else {
        None
    };
    BodyPreview {
        content_type,
        truncated,
        size: bytes.len() as u64,
        encoding,
        pretty,
        text,
        base64,
    }
}

pub fn preview_request_body(
    content_type: Option<&str>,
    content_encoding: Option<&str>,
    bytes: &[u8],
) -> BodyPreview {
    preview_body_with_encoding(content_type, content_encoding, bytes, DEFAULT_REQUEST_LIMIT)
}

pub fn preview_response_body(
    content_type: Option<&str>,
    content_encoding: Option<&str>,
    bytes: &[u8],
) -> BodyPreview {
    preview_body_with_encoding(
        content_type,
        content_encoding,
        bytes,
        DEFAULT_RESPONSE_LIMIT,
    )
}

pub fn preview_websocket_frame(
    direction: impl Into<String>,
    opcode: impl Into<String>,
    bytes: &[u8],
    max_bytes: usize,
) -> WebSocketFramePreview {
    let truncated = bytes.len() > max_bytes;
    let visible = if truncated {
        &bytes[..max_bytes]
    } else {
        bytes
    };
    let opcode = opcode.into();
    let text = if opcode.eq_ignore_ascii_case("text") {
        String::from_utf8(visible.to_vec()).ok()
    } else {
        None
    };
    let base64 = if text.is_none() && !visible.is_empty() {
        Some(STANDARD.encode(visible))
    } else {
        None
    };
    WebSocketFramePreview {
        direction: direction.into(),
        opcode,
        size: bytes.len() as u64,
        text,
        base64,
        truncated,
    }
}

fn pretty_body(content_type: Option<&str>, text: Option<&str>, visible: &[u8]) -> Option<String> {
    let text = text?;
    if is_json_like(content_type, visible) {
        if let Ok(value) = serde_json::from_str::<Value>(text) {
            return serde_json::to_string_pretty(&value).ok();
        }
    }
    if is_html_like(content_type) {
        return Some(pretty_html(text));
    }
    Some(text.to_string())
}

fn is_textual(content_type: Option<&str>, bytes: &[u8]) -> bool {
    if let Some(content_type) = content_type {
        is_text_like(Some(content_type)) || is_json_like(Some(content_type), bytes)
    } else {
        std::str::from_utf8(bytes).is_ok()
    }
}

fn is_text_like(content_type: Option<&str>) -> bool {
    let Some(content_type) = content_type else {
        return false;
    };
    let lower = content_type.to_ascii_lowercase();
    lower.starts_with("text/")
        || lower.contains("json")
        || lower.contains("xml")
        || lower.contains("html")
        || lower.contains("javascript")
        || lower.contains("x-www-form-urlencoded")
        || lower.contains("svg")
}

fn is_html_like(content_type: Option<&str>) -> bool {
    content_type
        .map(|value| value.to_ascii_lowercase().contains("html"))
        .unwrap_or(false)
}

fn is_json_like(content_type: Option<&str>, bytes: &[u8]) -> bool {
    if let Some(content_type) = content_type {
        if content_type.to_ascii_lowercase().contains("json") {
            return true;
        }
    }
    let trimmed = match std::str::from_utf8(bytes) {
        Ok(text) => text.trim_start(),
        Err(_) => return false,
    };
    trimmed.starts_with('{') || trimmed.starts_with('[')
}

fn pretty_html(text: &str) -> String {
    text.replace("><", ">\n<")
}
