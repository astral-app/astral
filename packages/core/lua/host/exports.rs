use core::ffi::{c_char, c_int};

use serde_json::{Value, json};

use crate::lua::host::events;
use crate::lua::host::http::{self as http, HttpRequest, HttpResponse};
use crate::lua::host::install::{decode_base64, encode_base64};
use crate::lua::host::nodes;
use crate::lua::host::ui;
use crate::lua::host::{self as host, Config, PlatformCall};
use crate::settings::store::AstralSettingsHandle;
use crate::support::bridge::{AstralRustString, bytes, optional, text};
use crate::support::host_callback::{AstralCallback, adopt_reply};

fn json_text(value: &Value) -> AstralRustString {
    AstralRustString::new(value.to_string().into_bytes())
}

fn parsed(data: *const c_char, length: usize) -> Value {
    serde_json::from_slice(bytes(data, length)).unwrap_or(Value::Null)
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

fn present(data: *const c_char, length: usize, flag: c_int) -> Option<Vec<u8>> {
    (flag != 0).then(|| bytes(data, length).to_vec())
}

fn headers_from(value: &Value) -> Vec<(String, String)> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|pair| Some((pair.get(0)?.as_str()?.to_owned(), pair.get(1)?.as_str()?.to_owned())))
        .collect()
}

fn headers_json(headers: &[(String, String)]) -> Value {
    Value::Array(headers.iter().map(|(name, value)| json!([name, value])).collect())
}

fn request_from(value: &Value) -> HttpRequest {
    HttpRequest {
        method: value.get("method").and_then(Value::as_str).unwrap_or_default().to_owned(),
        url: value.get("url").and_then(Value::as_str).unwrap_or_default().to_owned(),
        path: value.get("path").and_then(Value::as_str).unwrap_or_default().to_owned(),
        headers: headers_from(value.get("headers").unwrap_or(&Value::Null)),
        body: value
            .get("body_base64")
            .and_then(Value::as_str)
            .and_then(decode_base64)
            .unwrap_or_default(),
    }
}

fn request_json(request: &HttpRequest) -> Value {
    json!({
        "method": request.method, "url": request.url, "path": request.path,
        "headers": headers_json(&request.headers), "body_base64": encode_base64(&request.body),
    })
}

fn response_from(value: &Value) -> HttpResponse {
    HttpResponse {
        status: value.get("status").and_then(Value::as_i64).unwrap_or(200),
        headers: headers_from(value.get("headers").unwrap_or(&Value::Null)),
        body: value
            .get("body_base64")
            .and_then(Value::as_str)
            .and_then(decode_base64)
            .unwrap_or_default(),
    }
}

fn response_json(response: &HttpResponse) -> Value {
    json!({"status": response.status, "headers": headers_json(&response.headers), "body_base64": encode_base64(&response.body)})
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_lua_host_start(
    settings: *const AstralSettingsHandle,
    platform: Option<PlatformCall>,
    configuration: *const c_char,
    configuration_length: usize,
    start_enabled: c_int,
) {
    let configuration = parsed(configuration, configuration_length);
    let field = |name: &str| configuration.get(name).and_then(Value::as_str).unwrap_or_default().to_owned();
    let config = Config {
        preferred_languages: strings(configuration.get("preferredLanguages").unwrap_or(&Value::Null)),
        storage_path: field("storagePath"),
        discord_directory: field("discordDirectory"),
        bundled_directory: field("bundledDirectory"),
    };
    if !host::host().started {
        host::set_platform(platform);
    }
    host::start(
        strings(configuration.get("directories").unwrap_or(&Value::Null)),
        settings,
        start_enabled != 0,
        config,
    );
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_lua_host_stop() {
    host::stop();
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_lua_host_rescan() {
    host::rescan();
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_lua_host_plugins() -> AstralRustString {
    json_text(&host::plugins_json())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_lua_host_remove_storage(identifier: *const c_char, length: usize) {
    crate::lua::host::api::remove_storage(&text(identifier, length));
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_lua_host_set_enabled(identifier: *const c_char, length: usize, enabled: c_int) {
    host::set_enabled(&text(identifier, length), enabled != 0);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_lua_host_set_switch(
    identifier: *const c_char,
    length: usize,
    key: *const c_char,
    key_length: usize,
    on: c_int,
) {
    ui::set_switch(&text(identifier, length), &text(key, key_length), on != 0);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_lua_host_set_choice(
    identifier: *const c_char,
    length: usize,
    key: *const c_char,
    key_length: usize,
    value: *const c_char,
    value_length: usize,
) {
    ui::set_choice(&text(identifier, length), &text(key, key_length), &text(value, value_length));
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_lua_host_set_number(
    identifier: *const c_char,
    length: usize,
    key: *const c_char,
    key_length: usize,
    value: f64,
) {
    ui::set_number(&text(identifier, length), &text(key, key_length), value);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_lua_host_set_text(
    identifier: *const c_char,
    length: usize,
    key: *const c_char,
    key_length: usize,
    value: *const c_char,
    value_length: usize,
) {
    ui::set_text(&text(identifier, length), &text(key, key_length), &text(value, value_length));
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_lua_host_press_settings_button(
    identifier: *const c_char,
    length: usize,
    key: *const c_char,
    key_length: usize,
) {
    ui::press_settings_button(&text(identifier, length), &text(key, key_length));
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_lua_host_reset_settings(identifier: *const c_char, length: usize) {
    host::reset_settings(&text(identifier, length));
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_lua_host_complete_network_request(request: u64, response: *const c_char, length: usize) {
    host::complete_network_request(request, &parsed(response, length));
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_lua_host_complete_confirm(request: u64, accepted: c_int) {
    host::complete_confirm(request, accepted != 0);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_lua_host_sheet_event(
    sheet: u64,
    action: *const c_char,
    action_length: usize,
    value: *const c_char,
    value_length: usize,
    closed: c_int,
) {
    host::sheet_event(sheet, &text(action, action_length), &text(value, value_length), closed != 0);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_lua_host_complete_web_auth(
    request: u64,
    url: *const c_char,
    url_length: usize,
    has_url: c_int,
    cookies: *const c_char,
    cookies_length: usize,
    has_cookies: c_int,
    error: *const c_char,
    error_length: usize,
    has_error: c_int,
) {
    let lossy = |value: Option<Vec<u8>>| value.map(|value| String::from_utf8_lossy(&value).into_owned());
    host::complete_web_auth(
        request,
        lossy(present(url, url_length, has_url)),
        lossy(present(cookies, cookies_length, has_cookies)),
        lossy(present(error, error_length, has_error)),
    );
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_lua_host_complete_file_pick(
    request: u64,
    name: *const c_char,
    name_length: usize,
    has_name: c_int,
    data: *const c_char,
    data_length: usize,
    has_data: c_int,
    error: *const c_char,
    error_length: usize,
    has_error: c_int,
) {
    let error = present(error, error_length, has_error).map(|error| String::from_utf8_lossy(&error).into_owned());
    host::complete_file_pick(
        request,
        present(name, name_length, has_name).as_deref(),
        present(data, data_length, has_data).as_deref(),
        error.as_deref(),
    );
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_lua_host_emit_event(event: *const c_char, length: usize) {
    host::emit_event(&text(event, length));
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_lua_host_process_gateway_message(message: *const c_char, length: usize) -> AstralRustString {
    optional(events::process_gateway_message(bytes(message, length)).map(String::into_bytes))
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_lua_host_needs_gateway_events() -> c_int {
    c_int::from(events::needs_gateway_events())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_lua_host_wants_http(
    method: *const c_char,
    method_length: usize,
    host_name: *const c_char,
    host_length: usize,
    path: *const c_char,
    path_length: usize,
) -> c_int {
    c_int::from(http::wants_http(
        &text(method, method_length),
        &text(host_name, host_length),
        &text(path, path_length),
    ))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_lua_host_intercept_request(request: *const c_char, length: usize) -> AstralRustString {
    let mut request = request_from(&parsed(request, length));
    let decision = http::intercept_request(&mut request);
    json_text(&json!({
        "changed": decision.changed,
        "request": request_json(&request),
        "response": decision.response.as_ref().map(response_json),
    }))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_lua_host_intercept_response(
    request: *const c_char,
    request_length: usize,
    response: *const c_char,
    response_length: usize,
) -> AstralRustString {
    let request = request_from(&parsed(request, request_length));
    let mut response = response_from(&parsed(response, response_length));
    if !http::intercept_response(&request, &mut response) {
        return optional(None);
    }
    json_text(&response_json(&response))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_lua_host_press_message_action(identifier: *const c_char, length: usize) -> c_int {
    c_int::from(nodes::press_message_action(&text(identifier, length)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_lua_host_press_inserted_node(identifier: *const c_char, length: usize) -> c_int {
    c_int::from(nodes::press_inserted_node(&text(identifier, length)))
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_lua_host_set_menu_message(channel: u64, message: u64) {
    nodes::set_menu_message(channel, message);
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_lua_host_has_media_save_handlers() -> c_int {
    c_int::from(events::has_media_save_handlers())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_lua_host_resolve_media_save(save: *const c_char, length: usize, done: AstralCallback) -> u64 {
    let save = parsed(save, length);
    let field = |name: &str| save.get(name).and_then(Value::as_str).unwrap_or_default().to_owned();
    let media = events::MediaSave {
        url: field("url"),
        kind: crate::lua::enums::MediaKind::parse(&field("kind")).unwrap_or_default(),
        channel_id: field("channelID"),
        message_id: field("messageID"),
    };
    let done = adopt_reply(done).unwrap_or_else(|| std::sync::Arc::new(|_| {}));
    events::resolve_media_save(media, done)
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_lua_host_cancel_media_save(id: u64) {
    events::cancel_media_save(id);
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_lua_host_has_outgoing_message_transforms() -> c_int {
    c_int::from(events::has_outgoing_message_transforms())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_lua_host_transform_outgoing_message(
    channel: *const c_char,
    channel_length: usize,
    operation: *const c_char,
    operation_length: usize,
    content: *const c_char,
    content_length: usize,
) -> AstralRustString {
    optional(
        events::transform_outgoing_message(
            &text(channel, channel_length),
            &text(operation, operation_length),
            &text(content, content_length),
        )
        .map(String::into_bytes),
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_lua_host_composer_styles(text_bytes: *const c_char, length: usize) -> AstralRustString {
    json_text(&events::composer_styles(bytes(text_bytes, length)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_lua_host_eval(
    identifier: *const c_char,
    length: usize,
    code: *const c_char,
    code_length: usize,
) -> AstralRustString {
    AstralRustString::new(host::eval(&text(identifier, length), &text(code, code_length)).into_bytes())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_lua_host_reload(identifier: *const c_char, length: usize) {
    host::reload(&text(identifier, length));
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_lua_host_remove(identifier: *const c_char, length: usize) {
    host::remove(&text(identifier, length));
}
