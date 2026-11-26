use core::ffi::c_char;
use std::sync::OnceLock;

use regex_lite::Regex;

use crate::support::bridge::{AstralRustString, bytes};

const LIMIT: usize = 6000;

struct Patterns {
    token: Regex,
    mfa: Regex,
    secret: Regex,
    private_content: Regex,
    private_path: Regex,
    query: Regex,
}

fn patterns() -> &'static Patterns {
    static PATTERNS: OnceLock<Patterns> = OnceLock::new();
    PATTERNS.get_or_init(|| Patterns {
        token: Regex::new(r"[A-Za-z0-9_-]{20,}\.[A-Za-z0-9_-]{4,}\.[A-Za-z0-9_-]{20,}").expect("token pattern"),
        mfa: Regex::new(r"(?i)mfa\.[A-Za-z0-9_-]{20,}").expect("mfa pattern"),
        secret: Regex::new(
            r"(?i)((?:authorization|access_token|refresh_token|handoff_token|handoff_key|token|password|secret|session)\s*[:=]\s*)(?:bearer\s+)?[^\s,;]+",
        )
        .expect("secret pattern"),
        private_content: Regex::new(r"(?i)((?:message|content|body|cookie|set-cookie)\s*[:=]\s*)[^\r\n]*").expect("content pattern"),
        private_path: Regex::new(r"(?:file://)?(?:/Users/|/home/|/var/mobile/|/private/var/|/data/user/[0-9]+/|/storage/emulated/[0-9]+/)[^\s]+")
            .expect("path pattern"),
        query: Regex::new(r"(?i)(https?://[^\s?#]+)\?[^\s#]*").expect("query pattern"),
    })
}

fn truncated(text: &str, limit: usize) -> &str {
    let mut end = text.len().min(limit);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

pub fn sanitize(text: &[u8]) -> String {
    let lossy = String::from_utf8_lossy(&text[..text.len().min(LIMIT)]);
    let patterns = patterns();
    let mut sanitized = patterns.token.replace_all(&lossy, "[redacted]").into_owned();
    sanitized = patterns.mfa.replace_all(&sanitized, "[redacted]").into_owned();
    sanitized = patterns.secret.replace_all(&sanitized, "${1}[redacted]").into_owned();
    sanitized = patterns.private_content.replace_all(&sanitized, "${1}[redacted]").into_owned();
    sanitized = patterns.private_path.replace_all(&sanitized, "[private path]").into_owned();
    sanitized = patterns.query.replace_all(&sanitized, "${1}?[redacted]").into_owned();
    truncated(&sanitized, LIMIT).to_owned()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_sanitize_diagnostic_text(text: *const c_char, length: usize) -> AstralRustString {
    AstralRustString::new(sanitize(bytes(text, length)).into_bytes())
}
