use core::ffi::{c_char, c_int};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime};

use serde_json::value::RawValue;
use serde_json::{Map, Value, json};

use crate::support::bridge::{AstralRustString, bytes, path};
use crate::support::files::{list_directory, read_text, remove, write_atomically};
use crate::support::log::{ERROR, INFO, WARNING, log};

const CATEGORY: &str = "settings";
const CORRUPT_BACKUPS_KEPT: usize = 3;

#[derive(Clone, Copy, PartialEq, Default)]
struct Stamp {
    exists: bool,
    modified: Option<SystemTime>,
    size: u64,
}

pub struct Store {
    path: PathBuf,
    data: Map<String, Value>,
    raw: BTreeMap<String, Box<RawValue>>,
    stamp: Stamp,
    dirty: bool,
    journal: Vec<Value>,
    replaced: bool,
    unreadable: bool,
    recheck: Duration,
    checked_at: Option<Instant>,
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut joined = path.as_os_str().to_owned();
    joined.push(suffix);
    PathBuf::from(joined)
}

fn key_path(key: &str) -> Vec<&str> {
    key.split('.').collect()
}

fn remove_path(object: &mut Map<String, Value>, parts: &[&str]) -> bool {
    let Some((first, rest)) = parts.split_first() else { return false };
    if rest.is_empty() {
        return object.remove(*first).is_some();
    }
    let Some(Value::Object(child)) = object.get_mut(*first) else {
        return false;
    };
    if !remove_path(child, rest) {
        return false;
    }
    if child.is_empty() {
        object.remove(*first);
    }
    true
}

impl Store {
    pub fn new(path: PathBuf, recheck: Duration) -> Self {
        Store {
            path,
            data: Map::new(),
            raw: BTreeMap::new(),
            stamp: Stamp::default(),
            dirty: false,
            journal: Vec::new(),
            replaced: false,
            unreadable: false,
            recheck,
            checked_at: None,
        }
    }

    fn stamp_on_disk(&self) -> Stamp {
        match std::fs::metadata(&self.path) {
            Ok(metadata) if metadata.is_file() => Stamp {
                exists: true,
                modified: metadata.modified().ok(),
                size: metadata.len(),
            },
            _ => Stamp::default(),
        }
    }

    fn adopt(&mut self, contents: &[u8]) -> bool {
        let Ok(members) = serde_json::from_slice::<BTreeMap<String, Box<RawValue>>>(contents) else {
            return false;
        };
        self.data = Map::new();
        self.raw = members;
        true
    }

    fn refresh_locked(&mut self, before_write: bool) {
        if self.dirty {
            return;
        }
        let now = Instant::now();
        if !before_write && self.checked_at.is_some_and(|checked| now.duration_since(checked) < self.recheck) {
            return;
        }
        self.checked_at = Some(now);
        let current = self.stamp_on_disk();
        if current == self.stamp || !current.exists {
            return;
        }
        if let Some(contents) = read_text(&self.path) {
            self.adopt(&contents);
        }
        self.stamp = current;
    }

    fn parse_store(&mut self, store: &str) {
        let Some(raw) = self.raw.remove(store) else { return };
        match serde_json::from_str::<Value>(raw.get()) {
            Ok(value) => {
                self.data.insert(store.to_owned(), value);
            }
            Err(_) => log(
                ERROR,
                CATEGORY,
                &format!("Failed to parse settings store {store}; it reads as empty."),
            ),
        }
    }

    fn parse_all(&mut self) {
        while let Some(store) = self.raw.keys().next().cloned() {
            self.parse_store(&store);
        }
    }

    fn find(&mut self, store: &str, key: &str) -> Option<&Value> {
        self.refresh_locked(false);
        self.parse_store(store);
        let mut current = self.data.get(store).filter(|value| value.is_object())?;
        if key.is_empty() {
            return Some(current);
        }
        for part in key_path(key) {
            current = current.as_object()?.get(part)?;
        }
        (!current.is_null()).then_some(current)
    }

    fn set_locked(&mut self, store: &str, key: &str, value: Value) {
        self.dirty = true;
        self.journal.push(json!({"store": store, "key": key, "value": value}));
        self.parse_store(store);
        let payload = self.data.entry(store.to_owned()).or_insert_with(|| Value::Object(Map::new()));
        if !payload.is_object() {
            *payload = Value::Object(Map::new());
        }
        let parts = key_path(key);
        let mut current = payload;
        for part in &parts[..parts.len() - 1] {
            let object = current.as_object_mut().expect("settings path holds objects");
            let next = object.entry(part.to_string()).or_insert_with(|| Value::Object(Map::new()));
            if !next.is_object() {
                *next = Value::Object(Map::new());
            }
            current = next;
        }
        if let Some(object) = current.as_object_mut() {
            object.insert(parts[parts.len() - 1].to_owned(), value);
        }
    }

    fn remove_locked(&mut self, store: &str, key: &str) {
        self.parse_store(store);
        let Some(Value::Object(payload)) = self.data.get_mut(store) else {
            return;
        };
        if !remove_path(payload, &key_path(key)) {
            return;
        }
        let empty = payload.is_empty();
        self.dirty = true;
        self.journal.push(json!({"store": store, "key": key, "remove": true}));
        if empty {
            self.data.remove(store);
        }
    }

    fn apply_locked(&mut self, changes: &[Value]) {
        for change in changes {
            let Some(store) = change.get("store").and_then(Value::as_str) else {
                continue;
            };
            if change.get("clear").and_then(Value::as_bool) == Some(true) {
                let erased = self.data.remove(store).is_some() | self.raw.remove(store).is_some();
                if erased {
                    self.dirty = true;
                    self.journal.push(json!({"store": store, "clear": true}));
                }
                continue;
            }
            let Some(key) = change.get("key").and_then(Value::as_str) else {
                continue;
            };
            if change.get("remove").and_then(Value::as_bool) == Some(true) {
                self.remove_locked(store, key);
            } else {
                self.set_locked(store, key, change.get("value").cloned().unwrap_or(Value::Null));
            }
        }
    }

    fn dump(&self) -> String {
        let mut text = String::from("{");
        let mut append = |store: &str, value: &str| {
            if text.len() > 1 {
                text.push(',');
            }
            text.push_str(&Value::String(store.to_owned()).to_string());
            text.push(':');
            text.push_str(value);
        };
        for (store, value) in &self.data {
            append(store, &value.to_string());
        }
        for (store, raw) in &self.raw {
            append(store, raw.get());
        }
        text.push('}');
        text
    }

    fn reset(&mut self) {
        self.data = Map::new();
        self.raw.clear();
        if !self.unreadable {
            write_atomically(&self.path, b"{}");
        }
    }

    fn corrupt_prefix(&self) -> String {
        format!(
            "{}.corrupt-",
            self.path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
        )
    }

    fn prune_corrupt_backups(&self) {
        let Some(directory) = self.path.parent() else { return };
        let prefix = self.corrupt_prefix();
        let mut backups: Vec<(i64, Vec<u8>)> = list_directory(directory)
            .into_iter()
            .filter_map(|name| {
                let text = std::str::from_utf8(&name).ok()?;
                let stamp = text.strip_prefix(&prefix)?.parse::<i64>().ok()?;
                Some((stamp, name))
            })
            .collect();
        backups.sort();
        let excess = backups.len().saturating_sub(CORRUPT_BACKUPS_KEPT);
        for (_, name) in &backups[..excess] {
            remove(&directory.join(std::ffi::OsStr::new(std::str::from_utf8(name).unwrap_or_default())));
        }
    }

    fn backup_corrupt(&self) {
        let seconds = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs());
        let backup = with_suffix(&self.path, &format!(".corrupt-{seconds}"));
        if std::fs::copy(&self.path, &backup).is_err() {
            log(ERROR, CATEGORY, "Failed to back up corrupt settings.json.");
            return;
        }
        log(ERROR, CATEGORY, &format!("Backed up corrupt settings.json to {}", backup.display()));
        self.prune_corrupt_backups();
    }

    fn last_good(&self) -> PathBuf {
        with_suffix(&self.path, ".good")
    }

    fn keep_last_good(&self, contents: &[u8]) {
        if contents.len() <= 2 || read_text(&self.last_good()).as_deref() == Some(contents) {
            return;
        }
        if !write_atomically(&self.last_good(), contents) {
            log(WARNING, CATEGORY, "Failed to save the last good settings.json.");
        }
    }

    fn restore_last_good(&mut self) -> bool {
        remove(&with_suffix(&self.path, ".bin"));
        let Some(good) = read_text(&self.last_good()) else { return false };
        if !self.adopt(&good) || !write_atomically(&self.path, &good) {
            return false;
        }
        log(ERROR, CATEGORY, "settings.json was corrupt; restored the last good copy.");
        log(WARNING, "astral", "restored");
        true
    }

    pub fn load(&mut self) {
        let contents = read_text(&self.path);
        self.unreadable = contents.is_none() && self.path.exists();
        if self.unreadable {
            log(
                WARNING,
                CATEGORY,
                "settings.json exists but cannot be read, as before the first unlock; leaving it untouched.",
            );
        }
        let Some(contents) = contents else {
            self.reset();
            return;
        };
        if self.adopt(&contents) {
            self.keep_last_good(&contents);
        } else {
            self.backup_corrupt();
            if !self.restore_last_good() {
                log(ERROR, CATEGORY, "settings.json is corrupt and has no last good copy; resetting it.");
                log(WARNING, "astral", "reset");
                self.reset();
                return;
            }
        }
        self.stamp = self.stamp_on_disk();
        self.dirty = false;
        self.replaced = false;
        self.journal.clear();
    }

    fn merge_external(&mut self) {
        if self.replaced || !self.stamp.exists {
            return;
        }
        let current = self.stamp_on_disk();
        if !current.exists || current == self.stamp {
            return;
        }
        let Some(contents) = read_text(&self.path) else { return };
        if !self.adopt(&contents) {
            return;
        }
        let journal = std::mem::take(&mut self.journal);
        self.apply_locked(&journal);
        self.stamp = current;
        log(
            INFO,
            CATEGORY,
            "settings.json changed on disk while changes were pending; merged them.",
        );
    }

    pub fn save(&mut self) -> bool {
        if self.unreadable {
            return false;
        }
        self.merge_external();
        if !write_atomically(&self.path, self.dump().as_bytes()) {
            return false;
        }
        self.stamp = self.stamp_on_disk();
        self.dirty = false;
        self.replaced = false;
        self.journal.clear();
        true
    }

    pub fn reload(&mut self) {
        if !self.dirty && self.stamp.exists && self.stamp_on_disk() == self.stamp {
            return;
        }
        self.load();
    }

    pub fn refresh(&mut self) {
        if self.dirty && !self.unreadable {
            self.merge_external();
            return;
        }
        if self.stamp.exists && self.stamp_on_disk() == self.stamp {
            return;
        }
        self.load();
    }

    pub fn snapshot(&mut self) -> Value {
        self.refresh_locked(false);
        self.parse_all();
        Value::Object(self.data.clone())
    }

    pub fn replace(&mut self, json: &[u8]) -> bool {
        let Ok(Value::Object(parsed)) = serde_json::from_slice::<Value>(json) else {
            return false;
        };
        self.data = parsed;
        self.raw.clear();
        self.dirty = true;
        self.replaced = true;
        self.journal.clear();
        true
    }

    pub fn get(&mut self, store: &str, key: &str) -> Option<Value> {
        self.find(store, key).cloned()
    }

    pub fn get_boolean(&mut self, store: &str, key: &str, fallback: bool) -> bool {
        match self.find(store, key) {
            Some(Value::Bool(flag)) => *flag,
            Some(Value::Number(number)) => number.as_f64().is_some_and(|number| number != 0.0),
            Some(Value::String(text)) => matches!(text.as_str(), "true" | "1" | "yes"),
            _ => fallback,
        }
    }

    pub fn get_string(&mut self, store: &str, key: &str) -> Option<String> {
        self.find(store, key).and_then(Value::as_str).map(str::to_owned)
    }

    pub fn set(&mut self, store: &str, key: &str, value: Value) -> bool {
        self.refresh_locked(true);
        if self.find(store, key) == Some(&value) {
            return false;
        }
        self.set_locked(store, key, value);
        true
    }

    pub fn remove(&mut self, store: &str, key: &str) {
        self.refresh_locked(true);
        self.remove_locked(store, key);
    }

    pub fn stage(&mut self, changes: &[u8]) -> bool {
        let Ok(Value::Array(changes)) = serde_json::from_slice::<Value>(changes) else {
            return false;
        };
        self.refresh_locked(true);
        self.apply_locked(&changes);
        true
    }
}

pub struct AstralSettingsHandle(Mutex<Store>);

fn lock(handle: *const AstralSettingsHandle) -> Option<MutexGuard<'static, Store>> {
    let handle = unsafe { handle.as_ref() }?;
    Some(handle.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()))
}

pub unsafe fn store(handle: *const AstralSettingsHandle) -> Option<MutexGuard<'static, Store>> {
    lock(handle)
}

fn text<'a>(data: *const c_char, length: usize) -> &'a str {
    std::str::from_utf8(bytes(data, length)).unwrap_or("")
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_settings_open(file: *const c_char, length: usize, recheck_ms: u64) -> *mut AstralSettingsHandle {
    let store = Store::new(path(file, length).to_path_buf(), Duration::from_millis(recheck_ms));
    Box::into_raw(Box::new(AstralSettingsHandle(Mutex::new(store))))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_settings_close(handle: *mut AstralSettingsHandle) {
    if !handle.is_null() {
        drop(unsafe { Box::from_raw(handle) });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_settings_load(handle: *const AstralSettingsHandle) {
    if let Some(mut store) = lock(handle) {
        store.load();
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_settings_readable(handle: *const AstralSettingsHandle) -> c_int {
    lock(handle).map_or(0, |store| c_int::from(!store.unreadable))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_settings_reload(handle: *const AstralSettingsHandle) {
    if let Some(mut store) = lock(handle) {
        store.reload();
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_settings_refresh(handle: *const AstralSettingsHandle) {
    if let Some(mut store) = lock(handle) {
        store.refresh();
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_settings_save(handle: *const AstralSettingsHandle) -> c_int {
    lock(handle).map_or(0, |mut store| c_int::from(store.save()))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_settings_flush(handle: *const AstralSettingsHandle) -> c_int {
    lock(handle).map_or(0, |mut store| c_int::from(!store.dirty || store.save()))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_settings_snapshot(handle: *const AstralSettingsHandle, pretty: c_int) -> AstralRustString {
    let Some(mut store) = lock(handle) else {
        return AstralRustString::new(b"{}".to_vec());
    };
    let snapshot = store.snapshot();
    let text = if pretty != 0 {
        serde_json::to_string_pretty(&snapshot)
    } else {
        serde_json::to_string(&snapshot)
    };
    AstralRustString::new(text.unwrap_or_else(|_| "{}".to_owned()).into_bytes())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_settings_replace(handle: *const AstralSettingsHandle, json: *const c_char, length: usize) -> c_int {
    lock(handle).map_or(0, |mut store| c_int::from(store.replace(bytes(json, length))))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_settings_get(
    handle: *const AstralSettingsHandle,
    store: *const c_char,
    store_length: usize,
    key: *const c_char,
    key_length: usize,
    into: *mut AstralRustString,
) -> c_int {
    let Some(mut settings) = lock(handle) else { return 0 };
    let Some(value) = settings.get(text(store, store_length), text(key, key_length)) else {
        return 0;
    };
    if let Some(into) = unsafe { into.as_mut() } {
        *into = AstralRustString::new(value.to_string().into_bytes());
    }
    1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_settings_get_boolean(
    handle: *const AstralSettingsHandle,
    store: *const c_char,
    store_length: usize,
    key: *const c_char,
    key_length: usize,
    fallback: c_int,
) -> c_int {
    lock(handle).map_or(fallback, |mut settings| {
        c_int::from(settings.get_boolean(text(store, store_length), text(key, key_length), fallback != 0))
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_settings_get_string(
    handle: *const AstralSettingsHandle,
    store: *const c_char,
    store_length: usize,
    key: *const c_char,
    key_length: usize,
    into: *mut AstralRustString,
) -> c_int {
    let Some(mut settings) = lock(handle) else { return 0 };
    let Some(value) = settings.get_string(text(store, store_length), text(key, key_length)) else {
        return 0;
    };
    if let Some(into) = unsafe { into.as_mut() } {
        *into = AstralRustString::new(value.into_bytes());
    }
    1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_settings_set(
    handle: *const AstralSettingsHandle,
    store: *const c_char,
    store_length: usize,
    key: *const c_char,
    key_length: usize,
    json: *const c_char,
    json_length: usize,
) -> c_int {
    let Ok(value) = serde_json::from_slice::<Value>(bytes(json, json_length)) else {
        return 0;
    };
    lock(handle).map_or(0, |mut settings| {
        c_int::from(settings.set(text(store, store_length), text(key, key_length), value))
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_settings_remove(
    handle: *const AstralSettingsHandle,
    store: *const c_char,
    store_length: usize,
    key: *const c_char,
    key_length: usize,
) {
    if let Some(mut settings) = lock(handle) {
        settings.remove(text(store, store_length), text(key, key_length));
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_settings_stage(handle: *const AstralSettingsHandle, changes: *const c_char, length: usize) -> c_int {
    lock(handle).map_or(0, |mut settings| c_int::from(settings.stage(bytes(changes, length))))
}
