use core::ffi::{c_char, c_int};

use serde_json::{Map, Value, json};

use crate::bridge::{AstralRustString, bytes};
use crate::marketplace::current_catalog;
use crate::settings::{AstralSettingsHandle, Store, store as settings_store};

const FORMAT: i64 = 1;
const MAX_BACKUP: usize = 1 << 20;
const MAX_PLUGIN_ID: usize = 128;
const MAX_SETTING_KEY: usize = 48;
const SKIPPED_SETTING: &str = "enabledPlugin";

fn toml_value(value: &Value) -> Option<toml::Value> {
    match value {
        Value::Null => None,
        Value::Bool(flag) => Some(toml::Value::Boolean(*flag)),
        Value::Number(number) => number.as_i64().map(toml::Value::Integer).or_else(|| {
            if number.is_u64() {
                None
            } else {
                number.as_f64().map(toml::Value::Float)
            }
        }),
        Value::String(text) => Some(toml::Value::String(text.clone())),
        Value::Array(values) => Some(toml::Value::Array(values.iter().filter_map(toml_value).collect())),
        Value::Object(entries) => Some(toml::Value::Table(
            entries
                .iter()
                .filter_map(|(key, value)| Some((key.clone(), toml_value(value)?)))
                .collect(),
        )),
    }
}

fn identifier(value: &Value) -> Option<&str> {
    value.get("id").and_then(Value::as_str).filter(|id| !id.is_empty())
}

fn local_identifiers(values: &Value) -> Vec<Value> {
    values
        .as_array()
        .into_iter()
        .flatten()
        .filter(|value| !value.get("bundled").and_then(Value::as_bool).unwrap_or(false))
        .filter_map(|value| identifier(value).map(|id| Value::String(id.to_owned())))
        .collect()
}

pub fn export(store: &mut Store, version: &str, plugins: &Value, themes: &Value) -> String {
    let mut astral = store
        .snapshot()
        .get("astral")
        .filter(|value| value.is_object())
        .cloned()
        .unwrap_or_else(|| json!({}));
    if let Some(settings) = astral.as_object_mut() {
        settings.remove(SKIPPED_SETTING);
    }
    let mut plugin_settings = Map::new();
    for plugin in plugins.as_array().into_iter().flatten() {
        let Some(id) = identifier(plugin) else { continue };
        let mut entry = Map::new();
        entry.insert(
            "enabled".into(),
            Value::Bool(plugin.get("enabled").and_then(Value::as_bool).unwrap_or(false)),
        );
        let mut values = Map::new();
        for setting in plugin.get("switches").and_then(Value::as_array).into_iter().flatten() {
            let Some(key) = setting.get("key").and_then(Value::as_str).filter(|key| !key.is_empty()) else {
                continue;
            };
            if let Some(stored) = store.get("plugins", &format!("{id}.setting.{key}")) {
                values.insert(key.to_owned(), stored);
            }
        }
        if !values.is_empty() {
            entry.insert("settings".into(), Value::Object(values));
        }
        plugin_settings.insert(id.to_owned(), Value::Object(entry));
    }
    let backup = json!({
        "format": FORMAT,
        "astral": version,
        "settings": astral,
        "plugins": plugin_settings,
        "marketplace": {"plugins": local_identifiers(plugins), "themes": local_identifiers(themes)},
    });
    let table = toml_value(&backup).unwrap_or(toml::Value::Table(toml::Table::new()));
    format!("{}\n", toml::to_string(&table).unwrap_or_default().trim_end())
}

fn installed(values: &Value, wanted: &str) -> bool {
    values
        .as_array()
        .into_iter()
        .flatten()
        .any(|value| identifier(value) == Some(wanted))
}

fn missing(wanted: Option<&Value>, kind: &str, installed_items: &Value, catalog: &Value) -> Vec<Value> {
    let entries = catalog.get(format!("{kind}s")).and_then(Value::as_array);
    wanted
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|id| !id.is_empty() && !installed(installed_items, id))
        .filter_map(|id| entries?.iter().find(|entry| identifier(entry) == Some(id)).cloned())
        .map(|package| json!({"kind": kind, "package": package}))
        .collect()
}

pub fn import(store: &mut Store, contents: &[u8], plugins: &Value, themes: &Value) -> Result<Value, &'static str> {
    if contents.len() > MAX_BACKUP {
        return Err("backup exceeds 1 MiB");
    }
    let backup = std::str::from_utf8(contents)
        .ok()
        .and_then(crate::toml_json::parse)
        .ok_or("not an astral backup")?;
    if backup.get("format").and_then(Value::as_i64) != Some(FORMAT) {
        return Err("not an astral backup");
    }
    for (key, value) in backup.get("settings").and_then(Value::as_object).into_iter().flatten() {
        if key != SKIPPED_SETTING {
            store.set("astral", key, value.clone());
        }
    }
    for (plugin, entry) in backup.get("plugins").and_then(Value::as_object).into_iter().flatten() {
        if plugin.is_empty() || plugin.len() > MAX_PLUGIN_ID || !entry.is_object() {
            continue;
        }
        if let Some(enabled) = entry.get("enabled").filter(|value| value.is_boolean()) {
            store.set("plugins", &format!("{plugin}.enabled"), enabled.clone());
        }
        for (key, value) in entry.get("settings").and_then(Value::as_object).into_iter().flatten() {
            if !key.is_empty() && key.len() <= MAX_SETTING_KEY {
                store.set("plugins", &format!("{plugin}.setting.{key}"), value.clone());
            }
        }
    }
    store.save();
    let marketplace = backup.get("marketplace");
    let catalog = current_catalog();
    let mut installs = missing(marketplace.and_then(|value| value.get("plugins")), "plugin", plugins, &catalog);
    installs.extend(missing(
        marketplace.and_then(|value| value.get("themes")),
        "theme",
        themes,
        &catalog,
    ));
    Ok(Value::Array(installs))
}

fn parse(data: *const c_char, length: usize) -> Value {
    serde_json::from_slice(bytes(data, length)).unwrap_or(Value::Null)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_backup_export(
    settings: *const AstralSettingsHandle,
    version: *const c_char,
    version_length: usize,
    plugins: *const c_char,
    plugins_length: usize,
    themes: *const c_char,
    themes_length: usize,
) -> AstralRustString {
    let Some(mut store) = (unsafe { settings_store(settings) }) else {
        return AstralRustString::new(Vec::new());
    };
    let version = String::from_utf8_lossy(bytes(version, version_length)).into_owned();
    AstralRustString::new(export(&mut store, &version, &parse(plugins, plugins_length), &parse(themes, themes_length)).into_bytes())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_backup_import(
    settings: *const AstralSettingsHandle,
    contents: *const c_char,
    contents_length: usize,
    plugins: *const c_char,
    plugins_length: usize,
    themes: *const c_char,
    themes_length: usize,
    result: *mut AstralRustString,
) -> c_int {
    let Some(mut store) = (unsafe { settings_store(settings) }) else {
        return 0;
    };
    let outcome = import(
        &mut store,
        bytes(contents, contents_length),
        &parse(plugins, plugins_length),
        &parse(themes, themes_length),
    );
    let (ok, text) = match outcome {
        Ok(installs) => (1, installs.to_string()),
        Err(failure) => (0, failure.to_owned()),
    };
    if let Some(result) = unsafe { result.as_mut() } {
        *result = AstralRustString::new(text.into_bytes());
    }
    ok
}
