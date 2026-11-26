use core::ffi::c_char;
use std::sync::Mutex;

use crate::support::bridge::bytes;

const KEPT: [&str; 7] = [
    "user-agent",
    "x-super-properties",
    "x-discord-locale",
    "x-discord-timezone",
    "x-debug-options",
    "accept-language",
    "x-installation-id",
];
const DEFAULT_VERSION: u32 = 9;

#[derive(Clone, Default)]
struct Captured {
    version: Option<u32>,
    headers: Vec<(String, String)>,
}

static CAPTURED: Mutex<Option<Captured>> = Mutex::new(None);

fn is_discord_api(host: &str) -> bool {
    host == "discord.com" || host.ends_with(".discord.com")
}

fn version_of(path: &str) -> Option<u32> {
    path.strip_prefix("/api/v")?.split('/').next()?.parse().ok()
}

pub fn note(host: &str, path: &str, headers: &[(String, String)]) {
    if !is_discord_api(&host.to_ascii_lowercase()) || !path.starts_with("/api/") {
        return;
    }
    let kept: Vec<(String, String)> = headers
        .iter()
        .map(|(name, value)| (name.to_ascii_lowercase(), value.clone()))
        .filter(|(name, value)| KEPT.contains(&name.as_str()) && !value.is_empty())
        .collect();
    if !kept.iter().any(|(name, _)| name == "x-super-properties") {
        return;
    }
    *CAPTURED.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Captured {
        version: version_of(path),
        headers: kept,
    });
}

pub fn api_version() -> u32 {
    CAPTURED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_ref()
        .and_then(|captured| captured.version)
        .unwrap_or(DEFAULT_VERSION)
}

pub fn headers() -> Vec<(String, String)> {
    CAPTURED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_ref()
        .map(|captured| captured.headers.clone())
        .unwrap_or_default()
}

pub fn clear() {
    *CAPTURED.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
}

fn pairs_of(data: &[u8]) -> Vec<(String, String)> {
    let mut entries = Vec::new();
    let mut at = 0;
    while at + 4 <= data.len() {
        let length = u32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]]) as usize;
        at += 4;
        if length > data.len() - at {
            break;
        }
        entries.push(String::from_utf8_lossy(&data[at..at + length]).into_owned());
        at += length;
    }
    entries
        .as_chunks::<2>()
        .0
        .iter()
        .map(|[name, value]| (name.clone(), value.clone()))
        .collect()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_discord_note_request(
    host: *const c_char,
    host_length: usize,
    path: *const c_char,
    path_length: usize,
    headers: *const c_char,
    headers_length: usize,
) {
    let text = |data, length| String::from_utf8_lossy(bytes(data, length)).into_owned();
    note(
        &text(host, host_length),
        &text(path, path_length),
        &pairs_of(bytes(headers, headers_length)),
    );
}
