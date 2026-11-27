pub mod debug;
mod dev;
mod discord;
pub mod engine;
pub mod exchange;
mod ffi;
pub mod guest_event;
pub mod host;
pub mod launch;
pub mod network;
mod pages;
pub mod picked_media;
mod platform;
mod reports;
pub mod state;
mod theme;
pub mod web_auth;

use core::ffi::c_char;
use std::ffi::CString;
use std::sync::atomic::Ordering;

use serde_json::{Value, json};

use self::ffi::{collect, raw};
use self::host::Request;
use self::state::{App, BLOCKED_REQUESTS, PackKind, Platform, REFRESH_DECORATIONS, Setup, app, wire};
use crate::lua::enums::Event;
use crate::lua::host::http::{HttpDecision, HttpRequest, HttpResponse};
use crate::lua::host::{self as lua_host, Config};
use crate::wire::pages::AstralRowSpec;

pub use self::host::{Bridge, set_bridge};
pub use self::platform::picked_media_fetched;

const IDENTIFIER_CAPACITY: usize = 128;
const COMPOSER_EVENT_LIMIT: usize = 65_536;
const SLIDE_PAYLOAD_LIMIT: usize = 4096;
const SLIDER_PREFIX: &str = "astral.slider.";
const ROW_LINK: core::ffi::c_int = 0;

static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub struct Launch {
    pub platform: Platform,
    pub version: String,
    pub commit: String,
    pub property_names: Vec<String>,
    pub state_directory: String,
    pub discord_directory: String,
    pub resource_directory: String,
    pub languages: Vec<String>,
    pub discord_version: String,
    pub environment: Value,
}

fn now_seconds() -> i64 {
    std::time::UNIX_EPOCH
        .elapsed()
        .map_or(0, |elapsed| i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX))
}

fn resolve_properties(names: &[String]) -> usize {
    guest_event::remember(names);
    if names.is_empty() {
        return 0;
    }
    let owned: Vec<CString> = names.iter().filter_map(|name| CString::new(name.as_str()).ok()).collect();
    let pointers: Vec<*const c_char> = owned.iter().map(|name| name.as_ptr()).collect();
    let mut missing: *const c_char = core::ptr::null();
    let resolved = unsafe { crate::wire::properties::astral_wire_resolve_properties(pointers.as_ptr(), pointers.len(), &raw mut missing) };
    if !missing.is_null() {
        let name = unsafe { core::ffi::CStr::from_ptr(missing) }.to_string_lossy();
        host::log(&format!("Phoenix has no property {name}; astral keeps its defaults"));
    }
    resolved
}

static CATEGORY_STRINGS: std::sync::Mutex<Vec<CString>> = std::sync::Mutex::new(Vec::new());

fn install_category() {
    let category = crate::settings::pages::category();
    let rows = category.get("rows").and_then(Value::as_array).cloned().unwrap_or_default();
    let field = |row: &Value, key: &str| CString::new(row.get(key).and_then(Value::as_str).unwrap_or_default()).unwrap_or_default();
    let mut strings = CATEGORY_STRINGS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    strings.clear();
    let mut specs = Vec::with_capacity(rows.len());
    for row in &rows {
        let key = field(row, "key");
        let title = field(row, "title");
        let icon = field(row, "icon");
        specs.push(AstralRowSpec {
            key: key.as_ptr(),
            title: title.as_ptr(),
            subtitle: core::ptr::null(),
            detail: core::ptr::null(),
            icon: icon.as_ptr(),
            kind: ROW_LINK,
            on: 0,
            steps: 0,
            position: 0,
        });
        strings.extend([key, title, icon]);
    }
    let title = CString::new(category.get("title").and_then(Value::as_str).unwrap_or("astral")).unwrap_or_default();
    unsafe { crate::wire::pages::astral_wire_set_category(title.as_ptr(), specs.as_ptr(), specs.len()) };
    strings.push(title);
}

pub fn install_decorators() {
    use crate::{lua::host::hooks, lua::host::nodes, lua::host::spans, wire};
    wire::decorate::astral_wire_set_decorator(Some(nodes::astral_lua_decorate));
    wire::decorate::astral_wire_set_message_background_decorator(Some(nodes::astral_lua_message_background));
    wire::code_blocks::astral_wire_set_code_block_decorator(Some(nodes::astral_lua_code_block));
    wire::patch::astral_wire_set_node_patcher(Some(nodes::astral_lua_patch_node));
    wire::image_rewrite::astral_wire_set_image_rewriter(Some(nodes::astral_lua_image_rewrite));
    wire::host_calls::astral_host_calls_set_image_rewriter(Some(nodes::astral_lua_image_rewrite));
    wire::message_text::astral_wire_set_span_transformer(Some(spans::astral_lua_span_transform));
    wire::insert::astral_wire_set_node_inserter(Some(nodes::astral_lua_insert_nodes));
    wire::message_menu::astral_wire_set_message_menu_provider(Some(nodes::astral_lua_message_menu));
    wire::message_menu::astral_wire_set_popover_menu_provider(Some(nodes::astral_lua_popover_menu));
    wire::message_text::astral_wire_set_message_text_provider(Some(nodes::astral_lua_message_text));
    wire::composer::astral_wire_set_send_transform(Some(nodes::astral_lua_send_transform));
    wire::labels::astral_wire_set_relabeler(Some(hooks::astral_lua_relabel));
    wire::mentions::astral_wire_set_mention_decorator(Some(hooks::astral_lua_mention));
    wire::mentions::astral_wire_set_message_text_colorizer(Some(hooks::astral_lua_message_text_color));
    wire::names::astral_wire_set_name_colorizer(Some(nodes::astral_lua_name_color));
    wire::names::astral_wire_set_typing_decorator(Some(nodes::astral_lua_typing));
}

unsafe extern "C" fn resolve_emoji(codepoints: *const c_char, url: *mut c_char, capacity: usize) -> core::ffi::c_int {
    let Some(current) = app() else { return 0 };
    if codepoints.is_null() || url.is_null() {
        return 0;
    }
    let codepoints = unsafe { core::ffi::CStr::from_ptr(codepoints) }.to_string_lossy();
    let resolved = current.emoji_url(&codepoints);
    if resolved.is_empty() || resolved.len() >= capacity {
        return 0;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(resolved.as_ptr(), url.cast::<u8>(), resolved.len());
        *url.add(resolved.len()) = 0;
    }
    1
}

fn launch_environment(launch: &Launch) -> Value {
    let mut environment = if launch.environment.is_object() {
        launch.environment.clone()
    } else {
        json!({})
    };
    environment["astral"] = json!(launch.version);
    environment["discord"] = json!(launch.discord_version);
    environment
        .as_object_mut()
        .map(|fields| fields.entry("phoenix").or_insert(json!(launch.discord_version)));
    environment["engine"] = json!("Phoenix");
    environment
}

pub fn prepare(launch: Launch) -> &'static App {
    if let Some(current) = app() {
        return current;
    }
    install_category();
    let state = launch.state_directory.clone();
    let environment = launch_environment(&launch);
    let (directory, length) = raw(&state);
    unsafe { crate::diagnostics::reports::astral_reports_configure(directory, length) };
    let _ = std::fs::create_dir_all(format!("{state}/plugins"));
    let previous_exit = unsafe { crate::diagnostics::exit_record::astral_exit_record_take(directory, length, 0) };
    let previous_session = reports::open_trace(&state, now_seconds());
    let verdict = crate::diagnostics::launch_guard::begin(std::path::Path::new(&state), previous_exit);
    if !launch.discord_directory.is_empty() {
        crate::discord::cache::set_data_directory(&launch.discord_directory);
    }
    let current = state::install(Setup {
        platform: launch.platform,
        version: launch.version,
        commit: launch.commit,
        state,
        resources: launch.resource_directory,
        discord_directory: launch.discord_directory,
        discord_version: launch.discord_version,
        languages: launch.languages,
        recovering: verdict.enter_recovery != 0,
        environment,
    });
    if current.platform == Platform::Ios && previous_exit == crate::diagnostics::exit_record::EXIT_UNKNOWN {
        reports::record_unexpected_exit(previous_session);
    }
    theme::configure_marketplace(current);
    theme::apply(current, &theme::active(current));
    crate::wire::emoji::astral_wire_set_emoji_resolver(Some(resolve_emoji));
    current
}

pub fn start() -> usize {
    let Some(current) = app() else { return 0 };
    if !STARTED.swap(true, Ordering::AcqRel) {
        let safe_mode = current.flag("astral", "safe-mode", false);
        lua_host::set_platform(Some(platform::platform_call));
        lua_host::start(
            vec![format!("{}/plugins", current.state), format!("{}/plugins", current.resources)],
            current.settings_handle(),
            !safe_mode && !current.recovering,
            Config {
                preferred_languages: current.languages.clone(),
                storage_path: format!("{}/plugin-storage.sqlite", current.state),
                discord_directory: current.discord_directory.clone(),
                bundled_directory: current.resources.clone(),
            },
        );
        install_decorators();
        debug::register();
        pages::configure(current);
        dev::start(current);
    }
    lua_host::plugins().len()
}

pub fn initialize(launch: Launch) -> usize {
    let resolved = resolve_properties(&launch.property_names);
    let current = prepare(launch);
    let plugins = start();
    log_start(&current.version, resolved, plugins);
    resolved
}

pub fn initialize_in_background(launch: Launch) -> usize {
    let resolved = resolve_properties(&launch.property_names);
    let version = prepare(launch).version.clone();
    install_decorators();
    let spawned = std::thread::Builder::new().name("astral-start".into()).spawn(move || {
        let plugins = start();
        log_start(&version, resolved, plugins);
        lua_host::refresh_decorations();
    });
    if let Err(error) = spawned {
        host::log(&format!("Failed to start plugins in the background: {error}"));
    }
    resolved
}

fn log_start(version: &str, resolved: usize, plugins: usize) {
    host::log(&format!(
        "astral {version} resolved {resolved} Phoenix properties; {plugins} plugins"
    ));
}

fn guard_file(update: unsafe extern "C" fn(*const c_char, usize) -> core::ffi::c_int) {
    if let Some(current) = app() {
        let (directory, length) = raw(&current.state);
        unsafe { update(directory, length) };
    }
}

pub fn mark_visible() {
    guard_file(crate::diagnostics::launch_guard::astral_launch_guard_mark_shown);
    if STARTED.load(Ordering::Acquire) {
        guard_file(crate::diagnostics::exit_record::astral_exit_record_clear);
    }
}

pub fn mark_hidden() {
    guard_file(crate::diagnostics::launch_guard::astral_launch_guard_mark_hidden);
    guard_file(crate::diagnostics::exit_record::astral_exit_record_mark_ended);
}

pub fn mark_ready() {
    guard_file(crate::diagnostics::launch_guard::astral_launch_guard_succeed);
    guard_file(crate::diagnostics::exit_record::astral_exit_record_clear);
}

pub fn rewrite(ops: &[u8]) -> Option<Vec<u8>> {
    let _held = wire();
    let mut buffer = ffi::WireBuffer::new();
    if unsafe { crate::wire::rewrite::astral_settings_rewrite(ops.as_ptr(), ops.len(), buffer.pointer()) } == 0 {
        unsafe { crate::wire::identifiers::astral_wire_note_identifiers(ops.as_ptr(), ops.len()) };
        return None;
    }
    let bytes = buffer.bytes().to_vec();
    unsafe { crate::wire::identifiers::astral_wire_note_identifiers(bytes.as_ptr(), bytes.len()) };
    Some(bytes)
}

pub fn plugins() -> String {
    host::json_text(&lua_host::plugins_json())
}

pub fn set_enabled(identifier: &str, enabled: bool) {
    lua_host::set_enabled(identifier, enabled);
}

pub fn eval(identifier: &str, code: &str) -> String {
    lua_host::eval(identifier, code)
}

fn identifier_of(node: i32) -> Option<String> {
    let mut buffer = [0u8; IDENTIFIER_CAPACITY];
    let _held = wire();
    let found = unsafe { crate::wire::identifiers::astral_wire_identifier(node, buffer.as_mut_ptr().cast(), buffer.len()) };
    if found == 0 {
        return None;
    }
    let end = buffer.iter().position(|byte| *byte == 0).unwrap_or(buffer.len());
    Some(String::from_utf8_lossy(&buffer[..end]).into_owned())
}

pub fn press(node: i32) -> bool {
    let Some(identifier) = identifier_of(node) else { return false };
    if !crate::settings::navigator::handles(&identifier) {
        return false;
    }
    crate::settings::navigator::tap(&identifier);
    true
}

pub fn press_inserted_node(node: i32) -> bool {
    identifier_of(node).is_some_and(|identifier| crate::lua::host::nodes::press_inserted_node(&identifier))
}

pub fn press_message_action(node: i32) -> bool {
    identifier_of(node).is_some_and(|identifier| crate::lua::host::nodes::press_message_action(&identifier))
}

pub fn composer_event(event: &[u8]) -> Option<Vec<u8>> {
    if event.is_empty() || event.len() > COMPOSER_EVENT_LIMIT {
        return None;
    }
    let _held = wire();
    let mut before = ffi::WireBuffer::new();
    (unsafe { crate::wire::composer::astral_wire_composer_event(event.as_ptr(), event.len(), before.pointer()) } != 0)
        .then(|| before.bytes().to_vec())
}

pub fn touch_message(node: i32, row: i64) {
    let (message, channel) = {
        let _held = wire();
        let message = if row >= 0 {
            crate::wire::message_menu::astral_wire_message_for_row(node, row)
        } else {
            crate::wire::message_menu::astral_wire_message_for_node(node)
        };
        (message, crate::wire::ids::astral_wire_current_channel())
    };
    if message != 0 {
        crate::lua::host::nodes::set_menu_message(channel, message);
    }
}

pub fn pending_updates() -> Vec<u8> {
    use crate::wire;
    let _held = self::wire();
    let mut merged = Vec::new();
    collect(&mut merged, |buffer| unsafe { wire::commands::astral_wire_take_commands(buffer) });
    collect(&mut merged, |buffer| unsafe { wire::mentions::astral_wire_retry_mentions(buffer) });
    if REFRESH_DECORATIONS.swap(false, Ordering::AcqRel) || wire::patch::astral_wire_take_refresh_busy() != 0 {
        collect(&mut merged, |buffer| unsafe {
            wire::message_text::astral_wire_refresh_message_texts(buffer)
        });
        collect(&mut merged, |buffer| unsafe {
            wire::decorate::astral_wire_refresh_author_tags(buffer)
        });
        collect(&mut merged, |buffer| unsafe {
            wire::decorate::astral_wire_refresh_message_backgrounds(buffer)
        });
        collect(&mut merged, |buffer| unsafe {
            wire::code_blocks::astral_wire_refresh_code_blocks(buffer)
        });
        collect(&mut merged, |buffer| unsafe {
            wire::patch::astral_wire_refresh_node_patches(buffer)
        });
        collect(&mut merged, |buffer| unsafe {
            wire::insert::astral_wire_refresh_node_inserts(buffer)
        });
        if wire::patch::astral_wire_take_refresh_busy() != 0 {
            REFRESH_DECORATIONS.store(true, Ordering::Release);
        }
    }
    unsafe { wire::identifiers::astral_wire_note_identifiers(merged.as_ptr(), merged.len()) };
    merged
}

pub fn tweens_active() -> bool {
    let _held = wire();
    crate::wire::tween::astral_wire_tweens_active() != 0
}

pub fn tween_frame(now_ms: f64) -> Vec<u8> {
    let _held = wire();
    let mut bytes = Vec::new();
    collect(&mut bytes, |buffer| unsafe {
        crate::wire::tween::astral_wire_tween_frame(buffer, now_ms)
    });
    bytes
}

pub fn slide(node: i32, payload: &[u8]) -> bool {
    if payload.is_empty() || payload.len() > SLIDE_PAYLOAD_LIMIT {
        return false;
    }
    let Some(identifier) = identifier_of(node) else { return false };
    let mut position = 0.0;
    let decoded = {
        let _held = wire();
        unsafe { crate::wire::events::astral_wire_event_value(payload.as_ptr(), payload.len(), &raw mut position) }
    };
    if decoded == 0 || !identifier.starts_with(SLIDER_PREFIX) {
        return false;
    }
    crate::settings::navigator::slide_position(&identifier, position);
    true
}

pub fn back() -> bool {
    crate::settings::navigator::back()
}

pub fn page_open() -> bool {
    crate::settings::navigator::open()
}

pub fn refresh_decorations() {
    crate::lua::host::nodes::forget_tags();
    REFRESH_DECORATIONS.store(true, Ordering::Release);
}

pub struct CompletedRequest {
    pub request: u64,
    pub status: i64,
    pub ok: bool,
    pub url: String,
    pub headers: String,
    pub body: String,
    pub body_base64: Option<String>,
    pub error: Option<String>,
}

const MAX_STATUS: i64 = 999;
const HEADER_LIMIT: usize = 512 << 10;

pub fn complete_request(completed: CompletedRequest) {
    if completed.request == 0 {
        return;
    }
    let mut response = json!({
        "status": completed.status.clamp(0, MAX_STATUS),
        "ok": completed.ok,
        "url": completed.url,
        "body": completed.body,
    });
    if let Some(encoded) = completed.body_base64 {
        response["body_base64"] = json!(encoded);
    }
    if completed.headers.len() <= HEADER_LIMIT
        && let Ok(headers @ Value::Object(_)) = serde_json::from_str::<Value>(&completed.headers)
    {
        response["headers"] = headers;
    }
    if let Some(error) = completed.error {
        response["error"] = json!(error);
    }
    lua_host::complete_network_request(completed.request, &response);
}

pub fn sheet_event(sheet: u64, action: String, value: String, closed: bool) {
    if sheet == 0 {
        return;
    }
    std::thread::spawn(move || lua_host::sheet_event(sheet, &action, &value, closed));
}

pub fn complete_confirm(request: u64, accepted: bool) {
    if request != 0 {
        lua_host::complete_confirm(request, accepted);
    }
}

pub fn complete_web_auth(request: u64, url: Option<String>, cookies: Option<String>, error: Option<String>) {
    web_auth::end(request);
    if request != 0 {
        lua_host::complete_web_auth(request, url, cookies, error);
    }
}

pub fn complete_file_pick(request: u64, name: Option<&[u8]>, data: Option<&[u8]>, error: Option<&str>) {
    if request != 0 {
        lua_host::complete_file_pick(request, name, data, error);
    }
}

pub fn complete_text_edit(request: u64, value: Option<String>) {
    pages::complete_text_edit(request, value);
}

pub fn emit_event(name: &str) {
    lua_host::emit_event(name);
    if Event::parse(name) == Some(Event::AppForeground) {
        pages::refresh_catalog_if_due(false);
    }
}

pub fn record_exit_report(kind: &str, timestamp: i64, reason: i64, description: &str, trace: &str) {
    reports::record_process_exit(kind, timestamp, reason, description, trace);
}

pub fn export_backup() -> String {
    let Some(current) = app() else { return String::new() };
    theme::export_backup(current)
}

pub fn apply_backup(contents: &str) -> Option<String> {
    let Some(current) = app() else {
        return Some("astral is not ready".to_owned());
    };
    theme::apply_backup(current, contents)
}

pub fn wants_http(method: &str, host_name: &str, path: &str) -> bool {
    crate::lua::host::http::wants_http(method, host_name, path)
}

pub fn intercept_http_request(request: &mut HttpRequest) -> HttpDecision {
    crate::lua::host::http::intercept_request(request)
}

pub fn intercept_http_response(request: &HttpRequest, response: &mut HttpResponse) -> bool {
    crate::lua::host::http::intercept_response(request, response)
}

fn parsed_host_request(frame: &[u8]) -> Option<HttpRequest> {
    let parsed = crate::network::host_requests::parse(frame)?;
    Some(HttpRequest {
        method: parsed.method,
        url: parsed.url,
        path: parsed.path,
        headers: parsed.headers,
        body: parsed.body,
    })
}

fn rebuilt_host_request(frame: &[u8], request: &HttpRequest) -> Vec<u8> {
    crate::network::host_requests::rebuild(
        frame,
        &crate::network::host_requests::Request {
            method: request.method.clone(),
            url: request.url.clone(),
            path: request.path.clone(),
            headers: request.headers.clone(),
            body: request.body.clone(),
        },
    )
}

pub fn host_request_wanted(frame: &[u8]) -> bool {
    parsed_host_request(frame).is_some_and(|request| {
        crate::lua::host::http::wants_http(
            &request.method,
            &crate::network::host_requests::host_of(&request.url),
            &request.path,
        )
    })
}

const SINK_DROPPED_HEADERS: [&str; 3] = ["authorization", "cookie", "content-type"];

pub fn intercept_host_request(frame: &[u8]) -> Option<Vec<u8>> {
    let parsed = parsed_host_request(frame)?;
    let mut request = parsed.clone();
    let decision = crate::lua::host::http::intercept_request(&mut request);
    if let Some(response) = decision.response {
        let headers: Vec<(Vec<u8>, Vec<u8>)> = response
            .headers
            .iter()
            .map(|(name, value)| (name.clone().into_bytes(), value.clone().into_bytes()))
            .collect();
        let sink = String::from_utf8_lossy(&crate::network::sink::respond_with(
            i32::try_from(response.status).unwrap_or_default(),
            &headers,
            &response.body,
        ))
        .into_owned();
        if sink.is_empty() {
            return None;
        }
        let answered = HttpRequest {
            method: "GET".to_owned(),
            path: crate::network::host_requests::path_of(&sink),
            url: sink,
            body: Vec::new(),
            headers: parsed
                .headers
                .into_iter()
                .filter(|(name, _)| !SINK_DROPPED_HEADERS.contains(&name.as_str()))
                .collect(),
        };
        return Some(rebuilt_host_request(frame, &answered));
    }
    decision.changed.then(|| rebuilt_host_request(frame, &request))
}

pub fn rewrite_host_call(frame: &[u8]) -> Option<Vec<u8>> {
    let mut buffer = ffi::WireBuffer::new();
    (unsafe { crate::wire::host_calls::astral_host_call_rewrite(frame.as_ptr(), frame.len(), buffer.pointer()) } != 0)
        .then(|| buffer.bytes().to_vec())
}

pub fn needs_gateway_events() -> bool {
    crate::lua::host::events::needs_gateway_events()
}

pub fn process_gateway_message(message: &str) -> Option<String> {
    crate::lua::host::events::process_gateway_message(message.as_bytes())
}

pub fn is_telemetry(host_name: &str, path: &str) -> bool {
    crate::network::policy::is_telemetry_request(host_name, path)
}

pub fn is_current_user(host_name: &str, path: &str) -> bool {
    crate::network::policy::is_current_user_request(host_name, path)
}

pub fn staff_dev_tools_enabled() -> bool {
    app().is_some_and(|current| current.flag("astral", "staff-devtools", false))
}

pub fn with_staff_user(body: &str) -> Option<String> {
    staff_dev_tools_enabled()
        .then(|| crate::network::policy::with_staff_user(body.as_bytes()))
        .flatten()
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

pub fn with_staff_ready(message: &str) -> Option<String> {
    staff_dev_tools_enabled()
        .then(|| crate::network::policy::with_staff_ready(message.as_bytes()))
        .flatten()
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

pub fn note_blocked_request() {
    BLOCKED_REQUESTS.fetch_add(1, Ordering::Relaxed);
}

pub fn sink_url() -> String {
    crate::network::sink::url()
}

pub fn icon_path(name: &str) -> String {
    let path = theme::plugin_icon_path(name);
    if !path.is_empty() {
        return path;
    }
    app().map(|current| current.icon_path(name)).unwrap_or_default()
}

pub fn emoji_url(codepoints: &str) -> String {
    app().map(|current| current.emoji_url(codepoints)).unwrap_or_default()
}

pub fn composer_styles(value: &str) -> String {
    host::json_text(&crate::lua::host::events::composer_styles(value.as_bytes()))
}

fn refresh_emoji() {
    host::request(Request::RefreshEmoji);
}

fn refresh_images(_: &App) {
    let generation = state::IMAGE_GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
    let bytes = {
        let _held = wire();
        let mut bytes = Vec::new();
        collect(&mut bytes, |buffer| unsafe {
            crate::wire::images::astral_wire_refresh_images(buffer, generation)
        });
        bytes
    };
    host::deliver(&bytes);
}

pub(crate) fn soft_restart_without_reload(current: &App) {
    current.invalidate_packs();
    refresh_images(current);
    theme::apply(current, &theme::active(current));
    refresh_emoji();
    refresh_settings();
}

pub(crate) fn soft_restart(current: &App) {
    current.invalidate_packs();
    refresh_images(current);
    theme::apply(current, &theme::active(current));
    refresh_emoji();
    reload_enabled_plugins();
}

fn reload_enabled_plugins() -> usize {
    let plugins = lua_host::plugins_json();
    let enabled: Vec<String> = plugins
        .as_array()
        .into_iter()
        .flatten()
        .filter(|plugin| plugin.get("enabled").and_then(Value::as_bool) == Some(true))
        .filter_map(|plugin| plugin.get("id").and_then(Value::as_str).map(str::to_owned))
        .collect();
    for identifier in &enabled {
        lua_host::reload(identifier);
    }
    enabled.len()
}

fn select_pack(current: &App, kind: PackKind, identifier: &str) -> bool {
    if !current.select_pack(kind, identifier) {
        return false;
    }
    match kind {
        PackKind::Icons => refresh_images(current),
        PackKind::Emoji => refresh_emoji(),
    }
    true
}

pub fn refresh_settings() {
    crate::settings::navigator::refresh();
    host::request(Request::RefreshSettings);
}

pub fn restart_softly() -> usize {
    let Some(current) = app() else { return 0 };
    current.invalidate_packs();
    refresh_images(current);
    theme::apply(current, &theme::active(current));
    refresh_emoji();
    let plugins = reload_enabled_plugins();
    refresh_settings();
    plugins
}

pub fn page_category() -> String {
    host::json_text(&crate::settings::pages::category())
}

pub fn page_model(route: &str) -> String {
    host::json_text(&crate::settings::pages::model(route))
}

pub fn page_action(route: &str, key: &str) -> Option<String> {
    crate::settings::pages::action(route, key, std::sync::Arc::new(refresh_settings))
}

pub fn refresh_catalog(force: bool) {
    pages::refresh_catalog_if_due(force);
}

pub fn stop_list_reason(phoenix: &str) -> Option<String> {
    let (pointer, length) = raw(phoenix);
    ffi::take_optional(unsafe { crate::plugins::marketplace::astral_marketplace_stop_list_reason(pointer, length) })
}

pub fn complete_network_request(request: u64, response: &str) {
    if request == 0 {
        return;
    }
    let parsed: Value = serde_json::from_str(response).unwrap_or_else(|_| json!({ "error": "unreadable response" }));
    lua_host::complete_network_request(request, &parsed);
}

pub fn press_inserted_identifier(identifier: &str) -> bool {
    crate::lua::host::nodes::press_inserted_node(identifier)
}

pub fn press_message_identifier(identifier: &str) -> bool {
    crate::lua::host::nodes::press_message_action(identifier)
}

pub fn set_menu_message(channel: u64, message: u64) {
    if message != 0 {
        crate::lua::host::nodes::set_menu_message(channel, message);
    }
}

pub fn emoji_animated() -> bool {
    app().is_some_and(App::emoji_animated)
}

pub fn developer_log(line: &str) {
    crate::support::dev_server::append_log(line);
}

pub fn decorations_pending() -> bool {
    REFRESH_DECORATIONS.load(Ordering::Acquire)
}

pub fn custom_emoji_pack() -> bool {
    app().is_some_and(|current| current.active_pack(PackKind::Emoji) != state::DEFAULT_PACK)
}

pub fn record_diagnostics(payload: &str, images: &str) -> usize {
    let images: Value = serde_json::from_str(images).unwrap_or(Value::Null);
    let text = |key: &str| images.get(key).and_then(Value::as_str).unwrap_or_default().to_owned();
    crate::diagnostics::metrickit::set_app_images(crate::diagnostics::metrickit::AppImages {
        loaded: images
            .get("loaded")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        executable: text("executable"),
        frameworks: text("frameworks"),
    });
    reports::record_diagnostics(&serde_json::from_str(payload).unwrap_or(Value::Null))
}

pub fn record_https_check(check: &str) {
    crate::diagnostics::env_report::record_https_check(serde_json::from_str(check).unwrap_or(Value::Null));
}
