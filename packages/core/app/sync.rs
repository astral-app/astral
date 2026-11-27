use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::{Value, json};

use super::host::{self, Reply, Request};
use super::state::app;
use crate::support::crypto::{hex, sha256};
use crate::support::files::{read_text, write_atomically};

const STATE_FILE: &str = "sync.json";
const MARKER: &str = "\n[sync]\n";

static LOCK: Mutex<()> = Mutex::new(());

#[derive(Default)]
struct SyncState {
    device: String,
    applied_ms: i64,
}

fn state_path() -> Option<PathBuf> {
    app().map(|current| PathBuf::from(&current.state).join(STATE_FILE))
}

fn load() -> SyncState {
    let saved = state_path()
        .and_then(|path| read_text(&path))
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .unwrap_or(Value::Null);
    let mut state = SyncState {
        device: saved.get("device").and_then(Value::as_str).unwrap_or_default().to_owned(),
        applied_ms: saved.get("appliedMs").and_then(Value::as_i64).unwrap_or(0),
    };
    if state.device.is_empty() {
        let seed = format!("{}:{}", crate::support::bridge::epoch_ms(), std::process::id());
        state.device = hex(&sha256(seed.as_bytes()))[..16].to_owned();
        save(&state);
    }
    state
}

fn save(state: &SyncState) {
    if let Some(path) = state_path() {
        let text = json!({"device": state.device, "appliedMs": state.applied_ms}).to_string();
        write_atomically(&path, text.as_bytes());
    }
}

pub struct Stamp {
    pub device: String,
    pub updated_ms: i64,
}

pub fn stamp_of(contents: &str) -> Option<Stamp> {
    let footer = &contents[contents.rfind(MARKER)? + MARKER.len()..];
    let table: toml::Table = footer.parse().ok()?;
    Some(Stamp {
        device: table.get("device")?.as_str()?.to_owned(),
        updated_ms: table.get("updated_ms")?.as_integer()?,
    })
}

pub fn stamped(backup: &str, device: &str, updated_ms: i64) -> String {
    format!(
        "{}{MARKER}device = {}\nupdated_ms = {updated_ms}\n",
        backup.trim_end(),
        Value::String(device.to_owned())
    )
}

pub fn should_apply(stamp: &Stamp, device: &str, applied_ms: i64) -> bool {
    stamp.device != device && stamp.updated_ms > applied_ms
}

pub fn folder() -> Option<String> {
    match host::request(Request::SyncFolderName) {
        Reply::Text(Some(name)) if !name.is_empty() => Some(name),
        _ => None,
    }
}

pub fn push() {
    let Ok(_held) = LOCK.try_lock() else { return };
    if folder().is_none() {
        return;
    }
    let state = load();
    let backup = super::export_backup();
    if backup.is_empty() {
        return;
    }
    let contents = stamped(&backup, &state.device, crate::support::bridge::epoch_ms());
    host::request(Request::SyncWrite { contents: &contents });
}

pub fn pull() {
    let Ok(_held) = LOCK.try_lock() else { return };
    let Reply::Text(Some(contents)) = host::request(Request::SyncRead) else {
        return;
    };
    let mut state = load();
    let Some(stamp) = stamp_of(&contents).filter(|stamp| should_apply(stamp, &state.device, state.applied_ms)) else {
        return;
    };
    match super::apply_backup(&contents) {
        None => {
            state.applied_ms = stamp.updated_ms;
            save(&state);
            host::log(&format!("sync: applied settings from device {}", stamp.device));
            super::refresh_settings();
        }
        Some(error) => host::log(&format!("Failed to apply synced settings from {}: {error}", stamp.device)),
    }
}

pub fn choose() {
    host::request(Request::SyncChooseFolder);
}

pub fn chosen() {
    std::thread::spawn(|| {
        pull();
        push();
        super::refresh_settings();
    });
}

pub fn forget() {
    host::request(Request::SyncForgetFolder);
}
