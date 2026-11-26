#![allow(dead_code)]

use core::ffi::c_char;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use astral_core::lua::host::{self as lua_host, Config};
use astral_core::settings::store::{AstralSettingsHandle, astral_settings_open, store};
use astral_core::support::bridge::{AstralRustString, bytes};
use astral_core::support::host_callback::{AstralCallback, Reply, adopt_reply};
use serde_json::{Value, json};

pub mod batch;

const ADVERTISED: [&str; 7] = [
    "confirm",
    "openSheet",
    "pickFile",
    "webAuth",
    "networkRequest",
    "discordRequest",
    "dispatchGateway",
];
const SETTINGS_RECHECK_MS: u64 = 500;

pub fn work(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("astral-core-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("work directory");
    directory
}

pub fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().expect("parent")).expect("directories");
    std::fs::write(path, text).expect("write");
}

pub fn settings(root: &Path) -> *const AstralSettingsHandle {
    let path = root.join("settings.json").to_string_lossy().into_owned();
    unsafe { astral_settings_open(path.as_ptr().cast(), path.len(), SETTINGS_RECHECK_MS) }
}

pub fn set_setting(handle: *const AstralSettingsHandle, store_name: &str, key: &str, value: Value) {
    let mut guard = unsafe { store(handle) }.expect("settings");
    guard.set(store_name, key, value);
    guard.save();
}

pub fn setting_flag(handle: *const AstralSettingsHandle, store_name: &str, key: &str, fallback: bool) -> bool {
    unsafe { store(handle) }.expect("settings").get_boolean(store_name, key, fallback)
}

pub fn developer_settings(root: &Path) -> *const AstralSettingsHandle {
    let handle = settings(root);
    set_setting(handle, "astral", "developer-mode", json!(true));
    handle
}

pub type Handler = Box<dyn Fn(&Value) -> Value + Send + Sync>;

static PLATFORM: Mutex<BTreeMap<String, Arc<Handler>>> = Mutex::new(BTreeMap::new());

pub fn on(method: &str, handler: impl Fn(&Value) -> Value + Send + Sync + 'static) {
    PLATFORM
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(method.to_owned(), Arc::new(Box::new(handler)));
}

pub fn clear_platform() {
    PLATFORM.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clear();
}

fn reply(value: &Value) -> AstralRustString {
    AstralRustString::new(value.to_string().into_bytes())
}

unsafe extern "C" fn platform_call(
    method: *const c_char,
    method_length: usize,
    arguments: *const c_char,
    length: usize,
) -> AstralRustString {
    let name = String::from_utf8_lossy(bytes(method, method_length)).into_owned();
    let arguments: Value = serde_json::from_slice(bytes(arguments, length)).unwrap_or(Value::Null);
    let handlers = PLATFORM.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
    if name == "has" {
        let wanted = arguments[0].as_str().unwrap_or_default();
        return reply(&json!({ "value": ADVERTISED.contains(&wanted) && handlers.contains_key(wanted) }));
    }
    match handlers.get(&name) {
        Some(handler) => reply(&json!({ "value": handler(&arguments) })),
        None => reply(&json!({ "missing": true })),
    }
}

pub fn start_host(root: &Path, handle: *const AstralSettingsHandle, discord: Option<&Path>) {
    lua_host::set_platform(Some(platform_call));
    lua_host::start(
        vec![root.join("plugins").to_string_lossy().into_owned()],
        handle,
        true,
        Config {
            storage_path: root.join("storage.sqlite").to_string_lossy().into_owned(),
            discord_directory: discord.map(|path| path.to_string_lossy().into_owned()).unwrap_or_default(),
            ..Config::default()
        },
    );
}

pub fn stop_host() {
    lua_host::stop();
    clear_platform();
}

pub fn eval(plugin: &str, code: &str) -> String {
    lua_host::eval(plugin, code)
}

pub fn recorder<T: Send + 'static>() -> Arc<Mutex<Vec<T>>> {
    Arc::new(Mutex::new(Vec::new()))
}

pub fn recorded<T: Clone>(record: &Arc<Mutex<Vec<T>>>) -> Vec<T> {
    record.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
}

pub type PageHandler = Box<dyn Fn(&Value, Option<Reply>) -> Value + Send + Sync>;

static PAGES: Mutex<BTreeMap<String, Arc<PageHandler>>> = Mutex::new(BTreeMap::new());

pub fn on_page(method: &str, handler: impl Fn(&Value, Option<Reply>) -> Value + Send + Sync + 'static) {
    PAGES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(method.to_owned(), Arc::new(Box::new(handler)));
}

unsafe extern "C" fn pages_call(
    method: *const c_char,
    method_length: usize,
    arguments: *const c_char,
    length: usize,
    callback: AstralCallback,
) -> AstralRustString {
    let reply_to = adopt_reply(callback);
    let name = String::from_utf8_lossy(bytes(method, method_length)).into_owned();
    let arguments: Value = serde_json::from_slice(bytes(arguments, length)).unwrap_or(Value::Null);
    let handlers = PAGES.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
    if name == "capabilities" {
        let capabilities: serde_json::Map<String, Value> = handlers.keys().map(|key| (key.clone(), json!(true))).collect();
        return reply(&Value::Object(capabilities));
    }
    let result = match handlers.get(&name) {
        Some(handler) => handler(&arguments, reply_to),
        None => lua_host_default(&name, &arguments),
    };
    if result.is_null() {
        return AstralRustString {
            bytes: core::ptr::null_mut(),
            length: 0,
        };
    }
    reply(&result)
}

fn lua_host_default(method: &str, arguments: &Value) -> Value {
    use astral_core::lua::host::ui;
    let text = |index: usize| arguments[index].as_str().unwrap_or_default().to_owned();
    match method {
        "plugins" => return lua_host::plugins_json(),
        "setEnabled" => lua_host::set_enabled(&text(0), arguments[1].as_bool().unwrap_or(false)),
        "setSwitch" => ui::set_switch(&text(0), &text(1), arguments[2].as_bool().unwrap_or(false)),
        "setChoice" => ui::set_choice(&text(0), &text(1), &text(2)),
        "setNumber" => ui::set_number(&text(0), &text(1), arguments[2].as_f64().unwrap_or(0.0)),
        "setText" => ui::set_text(&text(0), &text(1), &text(2)),
        "pressSettingsButton" => ui::press_settings_button(&text(0), &text(1)),
        "resetSettings" => lua_host::reset_settings(&text(0)),
        "remove" => lua_host::remove(&text(0)),
        "rescan" => lua_host::rescan(),
        _ => {}
    }
    Value::Null
}

pub fn configure_pages(handle: *const AstralSettingsHandle, version: &str) {
    astral_core::settings::pages::configure(Some(pages_call), handle, version.to_owned());
}

pub fn reset_pages() {
    PAGES.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clear();
    astral_core::settings::pages::configure(None, core::ptr::null(), String::new());
}

pub fn changed() -> astral_core::support::host_callback::Changed {
    Arc::new(|| {})
}
