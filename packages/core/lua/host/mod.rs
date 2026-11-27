pub mod api;
pub mod events;
pub mod exports;
pub mod hooks;
pub mod http;
pub mod install;
pub mod nodes;
pub mod spans;
pub mod text_style;
pub mod timers;
pub mod ui;

use crate::lua::enums::Event;
use crate::plugins::capability::Capability;
use core::cell::{Cell, RefCell, UnsafeCell};
use core::ffi::{c_char, c_int};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

use serde_json::{Value, json};

use crate::lua::api::{self as lua, LuaState, NOREF};
use crate::lua::json::push_json;
use crate::lua::sandbox::{Sandbox, astral_lua_sandbox_free, astral_lua_sandbox_new};
use crate::plugins::package::{self as plugin_package, Limits, Package};
use crate::settings::store::{AstralSettingsHandle, Store};

const INSERT_PATH_SEPARATOR: char = '\u{1f}';
pub const MEMORY_LIMIT: usize = 8 << 20;
pub const SOURCE_LIMIT: usize = 256 << 10;
pub const MODULES_LIMIT: usize = 1 << 20;
pub const BUDGET_SLICES: c_int = 40;
pub const SLICE_INSTRUCTIONS: c_int = 5000;
pub const MAX_ERRORS: i32 = 5;
pub const MAX_DISPLAYED_TEXTS: usize = 512;
pub const MAX_DISPLAYED_TEXT: usize = 8000;

static LOCK: Mutex<()> = Mutex::new(());

thread_local! {
    static HELD: RefCell<Option<MutexGuard<'static, ()>>> = const { RefCell::new(None) };
    static DEPTH: Cell<usize> = const { Cell::new(0) };
}

pub struct Held;

pub fn lock() -> Held {
    if DEPTH.get() == 0 {
        let guard = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        HELD.with(|held| *held.borrow_mut() = Some(guard));
    }
    DEPTH.set(DEPTH.get() + 1);
    Held
}

const LOCK_PATIENCE: std::time::Duration = std::time::Duration::from_millis(4);
const LOCK_RETRY: std::time::Duration = std::time::Duration::from_micros(100);

fn lock_within(patience: std::time::Duration) -> Option<std::sync::MutexGuard<'static, ()>> {
    let deadline = std::time::Instant::now() + patience;
    loop {
        match LOCK.try_lock() {
            Ok(guard) => return Some(guard),
            Err(std::sync::TryLockError::Poisoned(poisoned)) => return Some(poisoned.into_inner()),
            Err(std::sync::TryLockError::WouldBlock) if std::time::Instant::now() < deadline => std::thread::sleep(LOCK_RETRY),
            Err(std::sync::TryLockError::WouldBlock) => return None,
        }
    }
}

pub fn try_lock() -> Option<Held> {
    if DEPTH.get() == 0 {
        let guard = lock_within(LOCK_PATIENCE)?;
        HELD.with(|held| *held.borrow_mut() = Some(guard));
    }
    DEPTH.set(DEPTH.get() + 1);
    Some(Held)
}

pub fn while_unlocked<T>(work: impl FnOnce() -> T) -> T {
    if DEPTH.get() != 1 {
        return work();
    }
    HELD.with(|held| drop(held.borrow_mut().take()));
    let result = work();
    let guard = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    HELD.with(|held| *held.borrow_mut() = Some(guard));
    result
}

impl Drop for Held {
    fn drop(&mut self) {
        DEPTH.set(DEPTH.get() - 1);
        if DEPTH.get() == 0 {
            HELD.with(|held| drop(held.borrow_mut().take()));
        }
    }
}

#[derive(Clone, Default)]
pub struct Option_ {
    pub value: String,
    pub title: String,
    pub subtitle: String,
}

#[derive(Clone, Default)]
pub struct Switch {
    pub key: String,
    pub title: String,
    pub subtitle: String,
    pub section: String,
    pub fallback: bool,
    pub options: Vec<Option_>,
    pub fallback_choice: String,
    pub number: bool,
    pub minimum: f64,
    pub maximum: f64,
    pub step: f64,
    pub fallback_number: f64,
    pub icon: String,
    pub order: usize,
}

#[derive(Clone, Default)]
pub struct TextSetting {
    pub key: String,
    pub title: String,
    pub subtitle: String,
    pub section: String,
    pub fallback: String,
    pub maximum_length: usize,
    pub color: bool,
    pub icon: String,
    pub order: usize,
}

pub struct SettingsButton {
    pub key: String,
    pub title: String,
    pub subtitle: String,
    pub section: String,
    pub icon: String,
    pub detail: String,
    pub order: usize,
    pub reference: c_int,
}

pub struct NodePatch {
    pub prefix: String,
    pub kind: String,
    pub reference: c_int,
}

pub struct NodeInsert {
    pub identifier: u64,
    pub prefix: String,
    pub kind: String,
    pub build: c_int,
    pub press: c_int,
}

pub struct EventCallback {
    pub event: crate::lua::enums::Event,
    pub reference: c_int,
    pub types: BTreeSet<String>,
}

pub struct MessageAction {
    pub identifier: u64,
    pub label: String,
    pub icon: String,
    pub danger: bool,
    pub reference: c_int,
    pub when: c_int,
}

pub struct HttpHook {
    pub method: String,
    pub pattern: String,
    pub request: c_int,
    pub response: c_int,
}

pub struct Command {
    pub name: String,
    pub description: String,
    pub options: Value,
    pub reference: c_int,
}

#[derive(Clone, Copy)]
pub struct Timer {
    pub reference: c_int,
    pub interval: i64,
}

#[derive(Default)]
pub struct Plugin {
    pub package: Package,
    pub permissions: BTreeSet<Capability>,
    pub errors: i32,
    pub failed: bool,
    pub last_error: String,
    pub bundled: bool,
    pub signed: bool,
    pub enabled: bool,
    pub sandbox: Option<core::ptr::NonNull<Sandbox>>,
    pub state: Option<core::ptr::NonNull<LuaState>>,
    pub switches: Vec<Switch>,
    pub settings_buttons: Vec<SettingsButton>,
    pub text_settings: Vec<TextSetting>,
    pub setting_count: usize,
    pub decorators: Vec<(crate::lua::enums::Target, c_int)>,
    pub relabelers: Vec<(crate::lua::enums::Target, c_int)>,
    pub text_stylers: Vec<c_int>,
    pub outgoing_message_transforms: Vec<c_int>,
    pub media_save_handlers: Vec<c_int>,
    pub composer_decorators: Vec<c_int>,
    pub message_background_decorators: Vec<c_int>,
    pub node_patches: Vec<NodePatch>,
    pub node_inserts: Vec<NodeInsert>,
    pub message_actions: Vec<MessageAction>,
    pub menu_actions: Vec<MessageAction>,
    pub network_requests: BTreeMap<u64, c_int>,
    pub timers: BTreeMap<u64, Timer>,
    pub web_auth_requests: BTreeMap<u64, c_int>,
    pub confirm_requests: BTreeMap<u64, c_int>,
    pub sheets: BTreeMap<u64, c_int>,
    pub file_picks: BTreeMap<u64, c_int>,
    pub event_callbacks: BTreeMap<u64, EventCallback>,
    pub http_hooks: Vec<HttpHook>,
    pub image_rewriters: Vec<(String, c_int)>,
    pub text_transforms: Vec<c_int>,
    pub commands: Vec<Command>,
}

impl Plugin {
    pub fn identifier(&self) -> &str {
        &self.package.identifier
    }

    pub fn state(&self) -> *mut LuaState {
        self.state.map_or(core::ptr::null_mut(), core::ptr::NonNull::as_ptr)
    }

    pub fn running(&self) -> bool {
        self.state.is_some() && !self.failed
    }

    pub fn reset_health(&mut self) {
        self.errors = 0;
        self.failed = false;
        self.last_error.clear();
    }

    pub fn mark_failed(&mut self) -> bool {
        if self.failed {
            return false;
        }
        self.failed = true;
        true
    }

    pub fn record_error(&mut self, stage: &str, message: &str) -> bool {
        self.last_error = format!("{stage}: {message}");
        self.errors += 1;
        self.errors >= MAX_ERRORS && self.mark_failed()
    }

    pub fn translate(&self, key: &str, values: &[(String, String)]) -> String {
        let text = self
            .package
            .strings
            .get(key)
            .or_else(|| self.package.fallback_strings.get(key))
            .map_or(key, String::as_str);
        plugin_package::fill_placeholders(text, values)
    }
}

pub type PlatformCall = unsafe extern "C" fn(
    method: *const c_char,
    method_length: usize,
    arguments: *const c_char,
    arguments_length: usize,
) -> crate::support::bridge::AstralRustString;

#[derive(Default)]
pub struct Config {
    pub preferred_languages: Vec<String>,
    pub storage_path: String,
    pub discord_directory: String,
    pub bundled_directory: String,
}

#[derive(Default)]
pub struct Host {
    pub plugins: Vec<*mut Plugin>,
    pub store: usize,
    pub started: bool,
    pub directories: Vec<String>,
    pub start_enabled: bool,
    pub config: Config,
    pub displayed_texts: BTreeMap<u64, (String, String)>,
    pub menu_channel: u64,
    pub menu_message: u64,
    pub popover_title: String,
    pub popover_guild: String,
    pub current_user_id: String,
    pub code_block_code: HashMap<String, String>,
}

struct HostCell(UnsafeCell<Option<Host>>);

unsafe impl Sync for HostCell {}

static HOST: HostCell = HostCell(UnsafeCell::new(None));
static PLATFORM: Mutex<Option<PlatformCall>> = Mutex::new(None);
pub static NEXT_REQUEST: AtomicU64 = AtomicU64::new(1);
pub static NEXT_MESSAGE_ACTION: AtomicU64 = AtomicU64::new(1);
pub static NEXT_INSERT: AtomicU64 = AtomicU64::new(1);
static REDRAW_AFTER_BUSY: AtomicBool = AtomicBool::new(false);

pub fn host() -> &'static mut Host {
    unsafe { (*HOST.0.get()).get_or_insert_with(Host::default) }
}

pub fn plugins() -> Vec<*mut Plugin> {
    host().plugins.clone()
}

pub fn next_request() -> u64 {
    let request = NEXT_REQUEST.fetch_add(1, Ordering::Relaxed);
    if request == 0 {
        NEXT_REQUEST.fetch_add(1, Ordering::Relaxed)
    } else {
        request
    }
}

pub fn find(identifier: &str) -> Option<*mut Plugin> {
    host()
        .plugins
        .iter()
        .copied()
        .find(|plugin| unsafe { (**plugin).identifier() } == identifier)
}

pub enum Platform {
    Missing,
    Threw,
    Value(Value),
}

pub fn set_platform(call: Option<PlatformCall>) {
    *PLATFORM.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = call;
}

pub fn platform(method: &str, arguments: &Value) -> Platform {
    let Some(call) = *PLATFORM.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) else {
        return Platform::Missing;
    };
    let arguments = arguments.to_string();
    let reply =
        crate::support::bridge::take(unsafe { call(method.as_ptr().cast(), method.len(), arguments.as_ptr().cast(), arguments.len()) });
    let reply: Value = serde_json::from_slice(&reply).unwrap_or(Value::Null);
    if reply.get("missing").and_then(Value::as_bool) == Some(true) {
        return Platform::Missing;
    }
    if reply.get("threw").and_then(Value::as_bool) == Some(true) {
        return Platform::Threw;
    }
    Platform::Value(reply.get("value").cloned().unwrap_or(Value::Null))
}

pub fn platform_value(method: &str, arguments: &Value) -> Option<Value> {
    match platform(method, arguments) {
        Platform::Value(value) => Some(value),
        _ => None,
    }
}

pub fn platform_flag(method: &str, arguments: &Value) -> bool {
    matches!(platform(method, arguments), Platform::Value(Value::Bool(true)))
}

pub fn platform_has(method: &str) -> bool {
    !matches!(
        platform("has", &json!([method])),
        Platform::Value(Value::Bool(false)) | Platform::Missing
    )
}

pub fn developer_log(line: &str) {
    platform("developerLog", &json!([line]));
}

pub fn refresh_decorations() {
    nodes::forget_tags();
    platform("refreshDecorations", &json!([]));
}

pub fn redraw_after_busy() {
    if REDRAW_AFTER_BUSY.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_millis(250));
        REDRAW_AFTER_BUSY.store(false, Ordering::SeqCst);
        refresh_decorations();
    });
}

pub fn with_store<T>(fallback: T, work: impl FnOnce(&mut Store) -> T) -> T {
    let handle = host().store as *const AstralSettingsHandle;
    match unsafe { crate::settings::store::store(handle) } {
        Some(mut store) => work(&mut store),
        None => fallback,
    }
}

pub fn setting_key(plugin: &Plugin, key: &str) -> String {
    format!("{}.setting.{key}", plugin.identifier())
}

pub fn stored(key: &str) -> Option<Value> {
    with_store(None, |store| store.get("plugins", key))
}

pub fn stored_string(key: &str, fallback: &str) -> String {
    stored(key)
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| fallback.to_owned())
}

pub fn stored_bool(key: &str, fallback: bool) -> bool {
    stored(key).and_then(|value| value.as_bool()).unwrap_or(fallback)
}

pub fn stored_number(key: &str, fallback: f64) -> f64 {
    stored(key).and_then(|value| value.as_f64()).unwrap_or(fallback)
}

pub fn store(key: &str, value: Value) {
    with_store((), |store| {
        store.set("plugins", key, value);
        store.save();
    });
}

pub fn stepped_number(entry: &Switch, value: f64) -> f64 {
    let clamped = value.max(entry.minimum).min(entry.maximum);
    let steps = ((clamped - entry.minimum) / entry.step).round();
    entry.maximum.min(entry.minimum + steps * entry.step)
}

pub fn record_failure(plugin: &Plugin) {
    platform(
        "recordFailure",
        &json!([plugin.identifier(), plugin.package.name, plugin.last_error]),
    );
}

pub(crate) fn mark_failed(plugin: *mut Plugin) {
    let plugin = unsafe { &mut *plugin };
    if plugin.mark_failed() {
        record_failure(plugin);
    }
}

pub(crate) fn report(plugin: *mut Plugin, stage: &str, message: &str) {
    let plugin = unsafe { &mut *plugin };
    let failed_now = plugin.record_error(stage, message);
    crate::support::log::log(
        crate::support::log::ERROR,
        "plugin",
        &format!("{} {}", plugin.identifier(), plugin.last_error),
    );
    developer_log(&format!("{} error: {}", plugin.identifier(), plugin.last_error));
    if failed_now {
        record_failure(plugin);
    }
}

fn trace_enter(identifier: &str) {
    unsafe { crate::plugins::trace::astral_trace_enter(identifier.as_ptr().cast(), identifier.len()) };
}

fn trace_leave() {
    crate::plugins::trace::astral_trace_leave();
}

pub(crate) fn call(plugin: *mut Plugin, arguments: c_int, results: c_int, stage: &str) -> bool {
    let (identifier, sandbox) = unsafe { ((*plugin).identifier().to_owned(), (*plugin).sandbox) };
    let Some(sandbox) = sandbox else { return false };
    trace_enter(&identifier);
    let error = unsafe { (*sandbox.as_ptr()).call(arguments, results) };
    trace_leave();
    match error {
        None => true,
        Some(error) => {
            report(plugin, stage, &String::from_utf8_lossy(&error));
            false
        }
    }
}

pub(crate) fn fail_and_stop(plugin: *mut Plugin) {
    mark_failed(plugin);
    stop_plugin(plugin);
}

pub(crate) fn call_or_stop(plugin: *mut Plugin, arguments: c_int, results: c_int, stage: &str) -> bool {
    if call(plugin, arguments, results, stage) {
        return true;
    }
    fail_and_stop(plugin);
    false
}

pub fn trace_running() {
    let running: Vec<Vec<u8>> = plugins()
        .into_iter()
        .filter(|plugin| unsafe { (**plugin).running() })
        .map(|plugin| unsafe { (*plugin).identifier().as_bytes().to_vec() })
        .collect();
    let joined = running.join(&0u8);
    unsafe { crate::plugins::trace::astral_trace_set_running(joined.as_ptr().cast(), joined.len()) };
}

fn release_all(state: *mut LuaState, references: impl IntoIterator<Item = c_int>) {
    for reference in references {
        unsafe { lua::release(state, reference) };
    }
}

pub(crate) fn stop_plugin(plugin: *mut Plugin) {
    let plugin_ref = unsafe { &mut *plugin };
    let state = plugin_ref.state();
    let timers = core::mem::take(&mut plugin_ref.timers);
    release_all(state, timers.values().map(|timer| timer.reference));
    for (request, reference) in core::mem::take(&mut plugin_ref.network_requests) {
        platform("cancelNetworkRequest", &json!([request]));
        unsafe { lua::release(state, reference) };
    }
    for (sheet, reference) in core::mem::take(&mut plugin_ref.sheets) {
        platform("closeSheet", &json!([sheet]));
        unsafe { lua::release(state, reference) };
    }
    release_all(state, core::mem::take(&mut plugin_ref.confirm_requests).into_values());
    for (request, reference) in core::mem::take(&mut plugin_ref.web_auth_requests) {
        platform("cancelWebAuth", &json!([request]));
        unsafe { lua::release(state, reference) };
    }
    for (request, reference) in core::mem::take(&mut plugin_ref.file_picks) {
        platform("cancelFilePick", &json!([request]));
        unsafe { lua::release(state, reference) };
    }
    crate::lua::host::http::release_hooks(plugin);
    let plugin_ref = unsafe { &mut *plugin };
    release_all(
        state,
        core::mem::take(&mut plugin_ref.image_rewriters)
            .into_iter()
            .map(|(_, reference)| reference),
    );
    release_all(
        state,
        core::mem::take(&mut plugin_ref.commands)
            .into_iter()
            .map(|command| command.reference),
    );
    release_all(state, core::mem::take(&mut plugin_ref.text_transforms));
    release_all(
        state,
        core::mem::take(&mut plugin_ref.event_callbacks)
            .into_values()
            .map(|callback| callback.reference),
    );
    release_all(
        state,
        core::mem::take(&mut plugin_ref.message_actions)
            .into_iter()
            .map(|action| action.reference),
    );
    release_all(
        state,
        core::mem::take(&mut plugin_ref.menu_actions)
            .into_iter()
            .map(|action| action.reference),
    );
    release_all(state, core::mem::take(&mut plugin_ref.outgoing_message_transforms));
    release_all(state, core::mem::take(&mut plugin_ref.media_save_handlers));
    if let Some(sandbox) = plugin_ref.sandbox.take() {
        unsafe { astral_lua_sandbox_free(sandbox.as_ptr()) };
    }
    plugin_ref.state = None;
    plugin_ref.switches.clear();
    plugin_ref.text_settings.clear();
    plugin_ref.settings_buttons.clear();
    plugin_ref.setting_count = 0;
    plugin_ref.decorators.clear();
    nodes::forget_tags();
    plugin_ref.relabelers.clear();
    plugin_ref.text_stylers.clear();
    plugin_ref.composer_decorators.clear();
    plugin_ref.message_background_decorators.clear();
    plugin_ref.node_patches.clear();
    sync_patch_prefixes();
    let plugin_ref = unsafe { &mut *plugin };
    plugin_ref.node_inserts.clear();
    sync_insert_prefixes();
    trace_running();
}

fn sync_prefixes(collect: impl Fn(&Plugin) -> Vec<String>, apply: unsafe extern "C" fn(*const *const c_char, usize)) {
    let mut prefixes: Vec<String> = Vec::new();
    for plugin in plugins() {
        let plugin = unsafe { &*plugin };
        if !plugin.running() {
            continue;
        }
        for prefix in collect(plugin) {
            if !prefixes.contains(&prefix) {
                prefixes.push(prefix);
            }
        }
    }
    let owned: Vec<std::ffi::CString> = prefixes
        .into_iter()
        .filter_map(|prefix| std::ffi::CString::new(prefix).ok())
        .collect();
    let pointers: Vec<*const c_char> = owned.iter().map(|prefix| prefix.as_ptr()).collect();
    unsafe { apply(pointers.as_ptr(), pointers.len()) };
}

pub fn sync_patch_prefixes() {
    sync_prefixes(
        |plugin| plugin.node_patches.iter().map(|patch| patch.prefix.clone()).collect(),
        crate::wire::patch::astral_wire_set_patch_prefixes,
    );
}

fn wire_insert_prefix(insert: &NodeInsert) -> String {
    let is_path = !insert.kind.is_empty() && insert.kind.bytes().all(|step| step == b'>' || step == b'^');
    if is_path {
        format!("{}{INSERT_PATH_SEPARATOR}{}", insert.prefix, insert.kind)
    } else {
        insert.prefix.clone()
    }
}

pub fn sync_insert_prefixes() {
    sync_prefixes(
        |plugin| plugin.node_inserts.iter().map(wire_insert_prefix).collect(),
        crate::wire::insert::astral_wire_set_insert_prefixes,
    );
}

fn runnable(plugin: &Plugin) -> bool {
    plugin.bundled || plugin.signed || with_store(false, |store| store.get_boolean("astral", "developer-mode", false))
}

pub(crate) fn start_plugin(plugin: *mut Plugin) -> bool {
    if !runnable(unsafe { &*plugin }) {
        stop_plugin(plugin);
        let plugin = unsafe { &mut *plugin };
        plugin.failed = true;
        plugin.last_error = "unsigned; turn on developer mode in astral's settings to run it".into();
        return false;
    }
    stop_plugin(plugin);
    let sandbox = astral_lua_sandbox_new(MEMORY_LIMIT, BUDGET_SLICES, SLICE_INSTRUCTIONS);
    let plugin_ref = unsafe { &mut *plugin };
    plugin_ref.reset_health();
    plugin_ref.sandbox = core::ptr::NonNull::new(sandbox);
    plugin_ref.state = core::ptr::NonNull::new(unsafe { crate::lua::sandbox::astral_lua_sandbox_state(sandbox) });
    if plugin_ref.state.is_none() {
        stop_plugin(plugin);
        return false;
    }
    crate::lua::host::install::install_api(plugin);
    crate::lua::host::install::install_require(plugin);
    if !run_prelude(plugin) {
        fail_and_stop(plugin);
        return false;
    }
    let plugin_ref = unsafe { &mut *plugin };
    let compiled = plugin_ref.signed && !plugin_ref.package.main_bytecode.is_empty();
    let identifier = plugin_ref.identifier().as_bytes().to_vec();
    let Some(sandbox) = plugin_ref.sandbox else { return false };
    let loaded = unsafe {
        if compiled {
            (*sandbox.as_ptr()).load(&plugin_ref.package.main_bytecode, &identifier, b"b\0")
        } else {
            (*sandbox.as_ptr()).load(&plugin_ref.package.source, &identifier, b"t\0")
        }
    };
    if let Some(error) = loaded {
        report(plugin, "load", &String::from_utf8_lossy(&error));
        fail_and_stop(plugin);
        return false;
    }
    if !call(plugin, 0, 0, "start") {
        fail_and_stop(plugin);
        return false;
    }
    trace_running();
    true
}

fn run_prelude(plugin: *mut Plugin) -> bool {
    let Some(sandbox) = (unsafe { (*plugin).sandbox }) else {
        return false;
    };
    let error = unsafe { (*sandbox.as_ptr()).load(crate::lua::host::install::prelude(), b"astral", b"b\0") };
    if let Some(error) = error {
        report(plugin, "prelude", &String::from_utf8_lossy(&error));
        return false;
    }
    call(plugin, 0, 0, "prelude")
}

fn load(directory: &str) -> Option<Box<Plugin>> {
    let limits = Limits {
        source: SOURCE_LIMIT,
        modules: MODULES_LIMIT,
        ..Limits::default()
    };
    let package = plugin_package::load(directory, &host().config.preferred_languages, limits)?;
    let signed = platform_flag("verifySignature", &json!([directory, package.main]));
    let permissions = package
        .permissions
        .iter()
        .filter_map(|permission| Capability::parse(permission))
        .collect();
    Some(Box::new(Plugin {
        package,
        permissions,
        signed,
        ..Plugin::default()
    }))
}

fn sorted_entries(root: &str) -> Vec<String> {
    let mut entries: Vec<String> = std::fs::read_dir(root)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    entries.sort();
    entries
}

pub fn scan() {
    let directories = host().directories.clone();
    let bundled_directory = host().config.bundled_directory.clone();
    for root in directories {
        let bundled = !bundled_directory.is_empty() && root.starts_with(&bundled_directory);
        for entry in sorted_entries(&root) {
            let Some(mut plugin) = load(&format!("{root}/{entry}")) else {
                continue;
            };
            if find(plugin.identifier()).is_some() {
                continue;
            }
            plugin.bundled = bundled;
            let enabled_key = format!("{}.enabled", plugin.identifier());
            let fallback = !bundled || plugin.package.enabled_by_default;
            let plugin = Box::into_raw(plugin);
            host().plugins.push(plugin);
            unsafe { (*plugin).enabled = stored_bool(&enabled_key, fallback) };
            if unsafe { (*plugin).enabled } && host().start_enabled {
                start_plugin(plugin);
            }
        }
    }
}

pub fn start(directories: Vec<String>, settings: *const AstralSettingsHandle, start_enabled: bool, config: Config) {
    let _held = lock();
    if host().started {
        return;
    }
    let host = host();
    host.started = true;
    host.store = settings as usize;
    host.directories = directories;
    host.start_enabled = start_enabled;
    host.config = config;
    if !host.config.discord_directory.is_empty() {
        crate::discord::cache::set_data_directory(&host.config.discord_directory);
    }
    crate::lua::host::api::set_storage_path(&host.config.storage_path);
    scan();
    refresh_decorations();
}

pub fn resume() -> usize {
    let _held = lock();
    if !host().started {
        return 0;
    }
    host().start_enabled = true;
    let mut started = 0;
    for plugin in plugins() {
        if unsafe { (*plugin).enabled && !(*plugin).running() } && start_plugin(plugin) {
            started += 1;
        }
    }
    drop(_held);
    refresh_decorations();
    started
}

pub fn rescan() {
    let _held = lock();
    if host().started {
        scan();
    }
}

pub fn stop() {
    let _held = lock();
    for plugin in plugins() {
        stop_plugin(plugin);
    }
    for plugin in core::mem::take(&mut host().plugins) {
        drop(unsafe { Box::from_raw(plugin) });
    }
    let host = host();
    host.started = false;
    host.store = 0;
    host.directories.clear();
    host.config = Config::default();
    host.start_enabled = false;
    set_platform(None);
}

pub fn plugins_json() -> Value {
    let _held = lock();
    let mut list = Vec::new();
    for plugin in plugins() {
        let plugin = unsafe { &*plugin };
        let switches: Vec<Value> = plugin
            .switches
            .iter()
            .map(|entry| {
                let options: Vec<Value> = entry
                    .options
                    .iter()
                    .map(|option| json!({"value": option.value, "title": option.title, "subtitle": option.subtitle}))
                    .collect();
                let key = setting_key(plugin, &entry.key);
                let value = if entry.number {
                    json!(stepped_number(entry, stored_number(&key, entry.fallback_number)))
                } else {
                    json!(stored_string(&key, &entry.fallback_choice))
                };
                json!({
                    "key": entry.key, "title": entry.title, "subtitle": entry.subtitle, "section": entry.section,
                    "on": stored_bool(&key, entry.fallback), "options": options, "value": value, "number": entry.number,
                    "min": entry.minimum, "max": entry.maximum, "step": entry.step, "icon": entry.icon, "order": entry.order,
                })
            })
            .collect();
        let text_settings: Vec<Value> = plugin
            .text_settings
            .iter()
            .map(|entry| {
                let mut value = stored_string(&setting_key(plugin, &entry.key), &entry.fallback);
                if value.len() > entry.maximum_length {
                    value = entry.fallback.clone();
                }
                json!({
                    "key": entry.key, "title": entry.title, "subtitle": entry.subtitle, "section": entry.section, "value": value,
                    "maxLength": entry.maximum_length, "color": entry.color, "icon": entry.icon, "order": entry.order,
                })
            })
            .collect();
        let buttons: Vec<Value> = plugin
            .settings_buttons
            .iter()
            .map(|entry| {
                json!({
                    "key": entry.key, "title": entry.title, "subtitle": entry.subtitle, "section": entry.section,
                    "icon": entry.icon, "detail": entry.detail, "button": true, "order": entry.order,
                })
            })
            .collect();
        let authors: Vec<Value> = plugin
            .package
            .authors
            .iter()
            .map(|(name, id)| json!({"name": name, "id": id}))
            .collect();
        list.push(json!({
            "permissions": plugin.permissions.iter().map(|capability| capability.value()).collect::<Vec<_>>(),
            "id": plugin.identifier(),
            "name": plugin.package.name,
            "version": plugin.package.version,
            "enabled": plugin.enabled,
            "running": plugin.state.is_some(),
            "failed": plugin.failed,
            "error": plugin.last_error,
            "switches": switches,
            "textSettings": text_settings,
            "settingsButtons": buttons,
            "description": plugin.package.description,
            "icon": plugin.package.icon,
            "iconSvg": plugin.package.icon_svg,
            "authors": authors,
            "directory": plugin.package.directory,
            "bundled": plugin.bundled,
            "signed": plugin.bundled || plugin.signed,
        }));
    }
    Value::Array(list)
}

pub fn plugin_list() -> Value {
    let _held = lock();
    let list: Vec<Value> = plugins()
        .into_iter()
        .map(|plugin| {
            let plugin = unsafe { &*plugin };
            json!({
                "id": plugin.identifier(), "name": plugin.package.name, "version": plugin.package.version,
                "enabled": plugin.enabled, "running": plugin.running(), "failed": plugin.failed,
            })
        })
        .collect();
    Value::Array(list)
}

pub fn set_enabled(identifier: &str, enabled: bool) {
    let _held = lock();
    let Some(plugin) = find(identifier) else { return };
    unsafe { (*plugin).enabled = enabled };
    store(&format!("{identifier}.enabled"), json!(enabled));
    if enabled {
        start_plugin(plugin);
    } else {
        stop_plugin(plugin);
    }
    refresh_decorations();
}

pub fn reset_settings(identifier: &str) {
    let _held = lock();
    let Some(plugin) = find(identifier) else { return };
    let keys: Vec<String> = unsafe {
        let plugin = &*plugin;
        plugin
            .switches
            .iter()
            .map(|entry| setting_key(plugin, &entry.key))
            .chain(plugin.text_settings.iter().map(|entry| setting_key(plugin, &entry.key)))
            .collect()
    };
    with_store((), |store| {
        for key in &keys {
            store.remove("plugins", key);
        }
        store.save();
    });
    if unsafe { (*plugin).state.is_some() } {
        start_plugin(plugin);
    }
}

pub fn take_callback(select: impl Fn(&mut Plugin) -> Option<c_int>) -> Option<(*mut Plugin, c_int)> {
    for plugin in plugins() {
        if let Some(reference) = select(unsafe { &mut *plugin }) {
            return Some((plugin, reference));
        }
    }
    None
}

pub unsafe fn push_callback(plugin: *mut Plugin, reference: c_int, release: bool) -> *mut LuaState {
    let state = unsafe { (*plugin).state() };
    unsafe {
        lua::push_reference(state, reference);
        if release {
            lua::release(state, reference);
        }
    }
    state
}

pub unsafe fn push_optional(state: *mut LuaState, value: Option<&str>) {
    match value {
        Some(value) => unsafe { lua::push_str(state, value) },
        None => unsafe { lua::lua_pushnil(state) },
    }
}

fn header_name_safe(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
}

fn header_value_safe(value: &str) -> bool {
    value.len() <= 8192 && value.bytes().all(|byte| !byte.is_ascii_control())
}

pub fn safe_header_name(name: &str) -> bool {
    header_name_safe(name)
}

pub fn safe_header_value(value: &str) -> bool {
    header_value_safe(value)
}

fn truncated(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_owned();
    }
    let mut end = limit;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

pub fn complete_network_request(request: u64, result: &Value) {
    let _held = lock();
    let Some((plugin, reference)) = take_callback(|plugin| plugin.network_requests.remove(&request)) else {
        return;
    };
    if !unsafe { (*plugin).running() } {
        return;
    }
    let object = result.as_object();
    let field = |name: &str| object.and_then(|object| object.get(name));
    let status = match field("status") {
        Some(value) if value.is_u64() => value.as_u64().unwrap_or(0).min(999) as i64,
        Some(value) if value.is_i64() => value.as_i64().unwrap_or(0).clamp(0, 999),
        _ => 0,
    };
    let mut response = json!({
        "status": status,
        "ok": field("ok").and_then(Value::as_bool).unwrap_or(false),
        "url": field("url").and_then(Value::as_str).map(|url| truncated(url, 4096)).unwrap_or_default(),
        "body": field("body").and_then(Value::as_str).map(|body| truncated(body, (1 << 20) + 1)).unwrap_or_default(),
    });
    if let Some(encoded) = field("body_base64").filter(|value| value.as_str().is_some_and(|text| text.len() <= 1400 << 10)) {
        response["body_base64"] = encoded.clone();
    }
    let mut headers = serde_json::Map::new();
    if let Some(entries) = field("headers").and_then(Value::as_object) {
        for (name, value) in entries {
            if headers.len() >= 64 {
                break;
            }
            if let Some(text) = value.as_str().filter(|text| header_name_safe(name) && header_value_safe(text)) {
                headers.insert(name.clone(), json!(truncated(text, 8192)));
            }
        }
    }
    response["headers"] = Value::Object(headers);
    let body = response["body"].as_str().unwrap_or_default().to_owned();
    if body.len() <= 64 << 10
        && !crate::support::json_bounds::nests_too_deeply(body.as_bytes(), 64)
        && let Ok(parsed) = serde_json::from_str::<Value>(&body)
    {
        response["json"] = parsed;
    }
    if let Some(error) = field("error").and_then(Value::as_str) {
        response["error"] = json!(truncated(error, 512));
    }
    if body.len() > 1 << 20 {
        response["body"] = json!("");
        response["ok"] = json!(false);
        response["error"] = json!("response body exceeds 1 MiB");
    }
    let state = unsafe { push_callback(plugin, reference, true) };
    unsafe { push_json(state, &response, 0) };
    call_or_stop(plugin, 1, 0, "network callback");
}

pub fn sheet_event(sheet: u64, action: &str, value: &str, closed: bool) {
    let _held = lock();
    let Some((plugin, reference)) = take_callback(|plugin| {
        if closed {
            plugin.sheets.remove(&sheet)
        } else {
            plugin.sheets.get(&sheet).copied()
        }
    }) else {
        return;
    };
    if !unsafe { (*plugin).running() } {
        return;
    }
    let state = unsafe { push_callback(plugin, reference, closed) };
    unsafe {
        lua::push_str(state, if closed { "closed" } else { action });
        lua::push_str(state, value);
    }
    call_or_stop(plugin, 2, 0, "sheet");
}

pub fn complete_confirm(request: u64, accepted: bool) {
    let _held = lock();
    let Some((plugin, reference)) = take_callback(|plugin| plugin.confirm_requests.remove(&request)) else {
        return;
    };
    if !unsafe { (*plugin).running() } {
        return;
    }
    let state = unsafe { push_callback(plugin, reference, true) };
    unsafe { lua::lua_pushboolean(state, c_int::from(accepted)) };
    call_or_stop(plugin, 1, 0, "confirm");
}

pub fn complete_web_auth(request: u64, url: Option<String>, cookies: Option<String>, error: Option<String>) {
    let _held = lock();
    let (mut url, mut cookies, mut error) = (url, cookies, error);
    if url.as_ref().is_some_and(|url| url.len() > 4096) {
        url = None;
        cookies = None;
        error = Some("authorization callback URL exceeds 4096 bytes".into());
    }
    if cookies.as_ref().is_some_and(|cookies| cookies.len() > 8192) {
        cookies = None;
        error = Some("authorization cookies exceed 8192 bytes".into());
    }
    let error = error.map(|error| truncated(&error, 512));
    let Some((plugin, reference)) = take_callback(|plugin| plugin.web_auth_requests.remove(&request)) else {
        return;
    };
    if !unsafe { (*plugin).running() } {
        return;
    }
    let state = unsafe { push_callback(plugin, reference, true) };
    unsafe {
        push_optional(state, url.as_deref());
        push_optional(state, cookies.as_deref());
        push_optional(state, error.as_deref());
    }
    call_or_stop(plugin, 3, 0, "web authorization callback");
}

pub fn complete_file_pick(request: u64, name: Option<&[u8]>, data: Option<&[u8]>, error: Option<&str>) {
    let _held = lock();
    let Some((plugin, reference)) = take_callback(|plugin| plugin.file_picks.remove(&request)) else {
        return;
    };
    if !unsafe { (*plugin).running() } {
        return;
    }
    let state = unsafe { push_callback(plugin, reference, true) };
    let picked = matches!((&name, &data), (Some(_), Some(data)) if data.len() <= 1 << 20);
    if picked {
        unsafe {
            lua::lua_createtable(state, 0, 2);
            lua::push_bytes(state, name.unwrap_or_default());
            lua::set_field(state, -2, "name");
            lua::push_bytes(state, data.unwrap_or_default());
            lua::set_field(state, -2, "data");
        }
    } else {
        let too_large = data.as_ref().is_some_and(|data| data.len() > 1 << 20);
        let fallback = if too_large { "file exceeds 1 MiB" } else { "cancelled" };
        let reason = error.unwrap_or(fallback);
        unsafe {
            lua::lua_pushnil(state);
            lua::push_bytes(state, &reason.as_bytes()[..reason.len().min(256)]);
        }
    }
    call_or_stop(plugin, if picked { 1 } else { 2 }, 0, "file picker callback");
}

pub fn emit_event(name: &str) {
    let Some(event @ (Event::AppForeground | Event::AppBackground)) = Event::parse(name) else {
        return;
    };
    let _held = lock();
    for plugin in plugins() {
        if !unsafe { (*plugin).running() } {
            continue;
        }
        let subscriptions: Vec<u64> = unsafe { &*plugin }
            .event_callbacks
            .iter()
            .filter(|(_, callback)| callback.event == event)
            .map(|(id, _)| *id)
            .collect();
        for subscription in subscriptions {
            let Some(reference) = (unsafe { &*plugin })
                .event_callbacks
                .get(&subscription)
                .map(|callback| callback.reference)
            else {
                continue;
            };
            if !unsafe { (*plugin).running() } {
                continue;
            }
            let state = unsafe { push_callback(plugin, reference, false) };
            unsafe { lua::push_str(state, event.value()) };
            if !call_or_stop(plugin, 1, 0, event.value()) {
                break;
            }
        }
    }
}

pub fn eval(identifier: &str, code: &str) -> String {
    let _held = lock();
    let Some(plugin) = find(identifier) else {
        return "error: no such plugin".into();
    };
    let Some(sandbox) = (unsafe { (*plugin).sandbox }) else {
        return "error: the plugin is not running".into();
    };
    let state = unsafe { (*plugin).state() };
    let top = unsafe { lua::lua_gettop(state) };
    let expression = [b"return ".as_slice(), code.as_bytes()].concat();
    if unsafe { (*sandbox.as_ptr()).load(&expression, b"eval", b"t\0") }.is_some()
        && let Some(error) = unsafe { (*sandbox.as_ptr()).load(code.as_bytes(), b"eval", b"t\0") }
    {
        unsafe { lua::lua_settop(state, top) };
        return format!("error: {}", String::from_utf8_lossy(&error));
    }
    trace_enter(identifier);
    let evaluated = unsafe { (*sandbox.as_ptr()).call(0, -1) };
    trace_leave();
    if let Some(error) = evaluated {
        unsafe { lua::lua_settop(state, top) };
        return format!("error: {}", String::from_utf8_lossy(&error));
    }
    let mut joined = String::new();
    let last = unsafe { lua::lua_gettop(state) };
    for index in top + 1..=last {
        if index > top + 1 {
            joined.push('\t');
        }
        let mut length = 0;
        let text = unsafe { lua::luaL_tolstring(state, index, &mut length) };
        joined.push_str(&String::from_utf8_lossy(unsafe {
            core::slice::from_raw_parts(text.cast::<u8>(), length)
        }));
        unsafe { lua::pop(state, 1) };
    }
    unsafe { lua::lua_settop(state, top) };
    joined
}

fn kept_state(plugin: Option<*mut Plugin>) -> Option<Value> {
    let plugin = plugin?;
    let state = unsafe { (*plugin).state() };
    if state.is_null() {
        return None;
    }
    let top = unsafe { lua::lua_gettop(state) };
    unsafe {
        lua::lua_getglobal(state, c"astral".as_ptr());
        if lua::is_table(state, -1) {
            lua::get_field(state, -1, "state");
        }
    }
    let kept = if unsafe { lua::is_table(state, -1) } {
        unsafe { crate::lua::json::to_json(state, -1, 0) }
    } else {
        None
    };
    unsafe { lua::lua_settop(state, top) };
    if kept.is_none() {
        developer_log(&format!(
            "{}: astral.state holds values that cannot be kept; reloading without it",
            unsafe { (*plugin).identifier() }
        ));
    }
    kept
}

fn restore_state(plugin: *mut Plugin, kept: &Value) {
    let state = unsafe { (*plugin).state() };
    if state.is_null() {
        return;
    }
    let top = unsafe { lua::lua_gettop(state) };
    unsafe { lua::lua_getglobal(state, c"astral".as_ptr()) };
    if !unsafe { lua::is_table(state, -1) } {
        unsafe { lua::lua_settop(state, top) };
        return;
    }
    unsafe {
        push_json(state, kept, 0);
        lua::set_field(state, -2, "state");
        lua::get_field(state, -1, "on_reload");
    }
    if unsafe { lua::is_function(state, -1) } {
        unsafe { lua::get_field(state, -2, "state") };
        call(plugin, 1, 0, "on_reload");
    }
    unsafe { lua::lua_settop(state, top) };
}

pub fn remove(identifier: &str) {
    let _held = lock();
    let Some(existing) = find(identifier) else { return };
    stop_plugin(existing);
    host().plugins.retain(|plugin| *plugin != existing);
    drop(unsafe { Box::from_raw(existing) });
    refresh_decorations();
}

pub fn reload(identifier: &str) {
    let _held = lock();
    let kept = kept_state(find(identifier));
    remove(identifier);
    rescan();
    set_enabled(identifier, true);
    if let (Some(plugin), Some(kept)) = (find(identifier), kept) {
        restore_state(plugin, &kept);
    }
}

pub(crate) fn plugin_of(state: *mut LuaState) -> *mut Plugin {
    unsafe { lua::lua_touserdata(state, lua::upvalue_index(1)).cast::<Plugin>() }
}

pub fn no_reference() -> c_int {
    NOREF
}
