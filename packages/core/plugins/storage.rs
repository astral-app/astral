use core::ffi::{c_char, c_int};
use std::sync::Mutex;

use serde_json::Value;

use crate::support::bridge::{AstralRustString, bytes};
use crate::support::sqlite_api::{self as sqlite, Database};

pub const SAVED: c_int = 0;
pub const REMOVED: c_int = 1;
pub const UNAVAILABLE: c_int = 2;
pub const OVER_LIMIT: c_int = 3;
pub const FAILED: c_int = 4;

pub struct Storage {
    database: Option<Mutex<Database>>,
    limit: usize,
}

impl Storage {
    pub fn open(path: &str, limit: usize) -> Storage {
        let database = Database::open(path, sqlite::OPEN_READWRITE | sqlite::OPEN_CREATE | sqlite::OPEN_FULLMUTEX).map(|database| {
            database.execute(
                "PRAGMA journal_mode=WAL;CREATE TABLE IF NOT EXISTS storage (plugin TEXT NOT NULL, key TEXT NOT NULL, value TEXT NOT NULL, PRIMARY KEY (plugin, key))",
            );
            Mutex::new(database)
        });
        Storage { database, limit }
    }

    fn with<T>(&self, fallback: T, work: impl FnOnce(&Database) -> T) -> T {
        match &self.database {
            Some(database) => work(&database.lock().unwrap_or_else(|poisoned| poisoned.into_inner())),
            None => fallback,
        }
    }

    pub fn available(&self) -> bool {
        self.database.is_some()
    }

    pub fn get(&self, plugin: &str, key: &str) -> Option<Value> {
        self.with(None, |database| {
            let mut statement = database.prepare(
                "SELECT value FROM storage WHERE plugin = ? AND key = ?",
                &[plugin.as_bytes(), key.as_bytes()],
            );
            if statement.step() != sqlite::ROW {
                return None;
            }
            serde_json::from_slice(&statement.text(0)).ok()
        })
    }

    fn bytes_except(database: &Database, plugin: &str, key: &str) -> usize {
        let mut statement = database.prepare(
            "SELECT COALESCE(SUM(LENGTH(key) + LENGTH(value)), 0) FROM storage WHERE plugin = ? AND key != ?",
            &[plugin.as_bytes(), key.as_bytes()],
        );
        if statement.step() == sqlite::ROW {
            statement.integer(0).max(0) as usize
        } else {
            0
        }
    }

    pub fn set(&self, plugin: &str, key: &str, value: &Value) -> c_int {
        if self.database.is_none() {
            return UNAVAILABLE;
        }
        if value.is_null() {
            return if self.remove(plugin, key) { REMOVED } else { FAILED };
        }
        let text = value.to_string();
        self.with(FAILED, |database| {
            if Self::bytes_except(database, plugin, key) + key.len() + text.len() > self.limit {
                return OVER_LIMIT;
            }
            let mut statement = database.prepare(
                "INSERT INTO storage (plugin, key, value) VALUES (?, ?, ?) ON CONFLICT (plugin, key) DO UPDATE SET value = excluded.value",
                &[plugin.as_bytes(), key.as_bytes(), text.as_bytes()],
            );
            if statement.step() == sqlite::DONE { SAVED } else { FAILED }
        })
    }

    pub fn remove(&self, plugin: &str, key: &str) -> bool {
        self.with(false, |database| {
            database
                .prepare(
                    "DELETE FROM storage WHERE plugin = ? AND key = ?",
                    &[plugin.as_bytes(), key.as_bytes()],
                )
                .step()
                == sqlite::DONE
        })
    }

    pub fn keys(&self, plugin: &str) -> Vec<String> {
        self.with(Vec::new(), |database| {
            let mut statement = database.prepare("SELECT key FROM storage WHERE plugin = ? ORDER BY key", &[plugin.as_bytes()]);
            let mut keys = Vec::new();
            while statement.step() == sqlite::ROW {
                keys.push(String::from_utf8_lossy(&statement.text(0)).into_owned());
            }
            keys
        })
    }

    pub fn remove_all(&self, plugin: &str) -> bool {
        self.with(false, |database| {
            database
                .prepare("DELETE FROM storage WHERE plugin = ?", &[plugin.as_bytes()])
                .step()
                == sqlite::DONE
        })
    }
}

fn text(data: *const c_char, length: usize) -> String {
    String::from_utf8_lossy(bytes(data, length)).into_owned()
}

fn storage<'a>(handle: *const Storage) -> Option<&'a Storage> {
    unsafe { handle.as_ref() }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_plugin_storage_open(path: *const c_char, length: usize, limit: usize) -> *mut Storage {
    Box::into_raw(Box::new(Storage::open(&text(path, length), limit)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_plugin_storage_close(handle: *mut Storage) {
    if !handle.is_null() {
        drop(unsafe { Box::from_raw(handle) });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_plugin_storage_available(handle: *const Storage) -> c_int {
    c_int::from(storage(handle).is_some_and(Storage::available))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_plugin_storage_get(
    handle: *const Storage,
    plugin: *const c_char,
    plugin_length: usize,
    key: *const c_char,
    key_length: usize,
) -> AstralRustString {
    match storage(handle).and_then(|storage| storage.get(&text(plugin, plugin_length), &text(key, key_length))) {
        Some(value) => AstralRustString::new(value.to_string().into_bytes()),
        None => AstralRustString {
            bytes: core::ptr::null_mut(),
            length: 0,
        },
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_plugin_storage_set(
    handle: *const Storage,
    plugin: *const c_char,
    plugin_length: usize,
    key: *const c_char,
    key_length: usize,
    value: *const c_char,
    value_length: usize,
) -> c_int {
    let Some(storage) = storage(handle) else { return UNAVAILABLE };
    let value: Value = serde_json::from_slice(bytes(value, value_length)).unwrap_or(Value::Null);
    storage.set(&text(plugin, plugin_length), &text(key, key_length), &value)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_plugin_storage_remove(
    handle: *const Storage,
    plugin: *const c_char,
    plugin_length: usize,
    key: *const c_char,
    key_length: usize,
) -> c_int {
    c_int::from(storage(handle).is_some_and(|storage| storage.remove(&text(plugin, plugin_length), &text(key, key_length))))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_plugin_storage_keys(
    handle: *const Storage,
    plugin: *const c_char,
    plugin_length: usize,
) -> AstralRustString {
    let keys = storage(handle)
        .map(|storage| storage.keys(&text(plugin, plugin_length)))
        .unwrap_or_default();
    AstralRustString::list(&keys.into_iter().map(String::into_bytes).collect::<Vec<_>>())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_plugin_storage_remove_all(handle: *const Storage, plugin: *const c_char, plugin_length: usize) -> c_int {
    c_int::from(storage(handle).is_some_and(|storage| storage.remove_all(&text(plugin, plugin_length))))
}
