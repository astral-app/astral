use core::ffi::{c_char, c_int, c_void};

use serde_json::Value;

use crate::bridge::{AstralRustString, bytes, optional};

const STAFF_FLAG: i64 = 1;
const DISCORD_DOMAINS: [&str; 2] = ["discord.com", "discordapp.com"];
const TRACKER_DOMAINS: [&str; 8] = [
    "sentry.io",
    "appsflyer.com",
    "appsflyersdk.com",
    "litix.io",
    "qualtrics.com",
    "app-measurement.com",
    "google-analytics.com",
    "crashlytics.com",
];
const MAX_BODY: usize = 1 << 20;
const MAX_CONTENT: usize = 20000;

fn is_under(host: &str, domain: &str) -> bool {
    host == domain || (host.len() > domain.len() && host.ends_with(domain) && host.as_bytes()[host.len() - domain.len() - 1] == b'.')
}

fn is_under_any(host: &str, domains: &[&str]) -> bool {
    domains.iter().any(|domain| is_under(host, domain))
}

fn without_query(path: &str) -> &str {
    path.split('?').next().unwrap_or(path)
}

fn api_route(path: &str) -> Option<&str> {
    let mut route = without_query(path).strip_prefix("/api")?;
    if route.len() > 2 && route.starts_with("/v") {
        let digits = route[2..].bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0 {
            route = &route[2 + digits..];
        }
    }
    while route.len() > 1 && route.ends_with('/') {
        route = &route[..route.len() - 1];
    }
    Some(route)
}

fn is_route_or_child(path: &str, route: &str) -> bool {
    path == route || (path.len() > route.len() && path.starts_with(route) && path.as_bytes()[route.len()] == b'/')
}

fn telemetry_route(path: &str) -> bool {
    let normalized = without_query(path).to_ascii_lowercase();
    if is_route_or_child(&normalized, "/error-reporting-proxy") {
        return true;
    }
    api_route(&normalized).is_some_and(|route| is_route_or_child(route, "/science") || is_route_or_child(route, "/metrics"))
}

pub fn is_discord_host(host: &str) -> bool {
    is_under_any(&host.to_ascii_lowercase(), &DISCORD_DOMAINS)
}

pub fn is_telemetry_request(host: &str, path: &str) -> bool {
    let host = host.to_ascii_lowercase();
    is_under_any(&host, &TRACKER_DOMAINS) || (is_under_any(&host, &DISCORD_DOMAINS) && telemetry_route(path))
}

pub fn is_current_user_request(host: &str, path: &str) -> bool {
    is_discord_host(host) && api_route(path) == Some("/users/@me")
}

pub fn outgoing_message_target(host: &str, path: &str, method: &str) -> Option<(String, &'static str)> {
    if !is_discord_host(host) {
        return None;
    }
    let route = api_route(path)?;
    let create = method == "POST";
    if !create && method != "PATCH" {
        return None;
    }
    let route = route.trim_start_matches('/');
    let segments: Vec<&str> = if route.is_empty() { Vec::new() } else { route.split('/').collect() };
    if segments.len() != if create { 3 } else { 4 } || segments[0] != "channels" || segments[2] != "messages" {
        return None;
    }
    let snowflake = |value: &str| !value.is_empty() && value.bytes().all(|character| character.is_ascii_digit());
    if !snowflake(segments[1]) || (!create && !snowflake(segments[3])) {
        return None;
    }
    Some((segments[1].to_owned(), if create { "create" } else { "edit" }))
}

fn mark_staff(user: &mut Value) -> bool {
    let Some(user) = user.as_object_mut() else { return false };
    for field in ["flags", "public_flags"] {
        let flags = user.get(field).and_then(Value::as_i64).unwrap_or(0);
        user.insert(field.to_owned(), Value::from(flags | STAFF_FLAG));
    }
    true
}

pub fn with_staff_user(body: &[u8]) -> Option<Vec<u8>> {
    let mut user: Value = serde_json::from_slice(body).ok()?;
    mark_staff(&mut user).then(|| user.to_string().into_bytes())
}

pub fn with_staff_ready(message: &[u8]) -> Option<Vec<u8>> {
    if !message.windows(7).any(|window| window == b"\"READY\"") {
        return None;
    }
    let mut payload: Value = serde_json::from_slice(message).ok()?;
    if payload.get("t").and_then(Value::as_str) != Some("READY") {
        return None;
    }
    let user = payload.get_mut("d").filter(|data| data.is_object())?.get_mut("user")?;
    mark_staff(user).then(|| payload.to_string().into_bytes())
}

pub type Transform =
    unsafe extern "C" fn(*mut c_void, *const c_char, usize, *const c_char, usize, *const c_char, usize, *mut AstralRustString) -> c_int;

fn text<'a>(data: *const c_char, length: usize) -> &'a str {
    std::str::from_utf8(bytes(data, length)).unwrap_or("")
}

pub fn transform_message_body(
    host: &str,
    path: &str,
    method: &str,
    body: &[u8],
    transform: impl FnOnce(&str, &str, &str) -> Option<String>,
) -> Option<Vec<u8>> {
    if body.is_empty() || body.len() > MAX_BODY {
        return None;
    }
    let (channel, operation) = outgoing_message_target(host, path, method)?;
    let mut request: Value = serde_json::from_slice(body).ok()?;
    let content = request.as_object()?.get("content")?.as_str()?.to_owned();
    if content.len() > MAX_CONTENT {
        return None;
    }
    let produced = transform(&channel, operation, &content).filter(|produced| produced.len() <= MAX_CONTENT)?;
    if produced == content {
        return None;
    }
    request["content"] = Value::String(produced);
    Some(request.to_string().into_bytes())
}

fn transform_body(host: &str, path: &str, method: &str, body: &[u8], transform: Transform, context: *mut c_void) -> Option<Vec<u8>> {
    transform_message_body(host, path, method, body, |channel, operation, content| {
        let mut replacement = AstralRustString {
            bytes: core::ptr::null_mut(),
            length: 0,
        };
        let changed = unsafe {
            transform(
                context,
                channel.as_ptr().cast(),
                channel.len(),
                operation.as_ptr().cast(),
                operation.len(),
                content.as_ptr().cast(),
                content.len(),
                &mut replacement,
            )
        };
        let produced = bytes(replacement.bytes.cast(), replacement.length).to_vec();
        unsafe { crate::bridge::astral_rust_string_free(replacement) };
        if changed == 0 {
            return None;
        }
        String::from_utf8(produced).ok()
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_is_discord_host(host: *const c_char, length: usize) -> c_int {
    c_int::from(is_discord_host(text(host, length)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_is_telemetry_request(
    host: *const c_char,
    host_length: usize,
    path: *const c_char,
    path_length: usize,
) -> c_int {
    c_int::from(is_telemetry_request(text(host, host_length), text(path, path_length)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_is_current_user_request(
    host: *const c_char,
    host_length: usize,
    path: *const c_char,
    path_length: usize,
) -> c_int {
    c_int::from(is_current_user_request(text(host, host_length), text(path, path_length)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_outgoing_message_target(
    host: *const c_char,
    host_length: usize,
    path: *const c_char,
    path_length: usize,
    method: *const c_char,
    method_length: usize,
    channel: *mut AstralRustString,
    operation: *mut AstralRustString,
) -> c_int {
    let Some((found, kind)) = outgoing_message_target(text(host, host_length), text(path, path_length), text(method, method_length)) else {
        return 0;
    };
    if let Some(channel) = unsafe { channel.as_mut() } {
        *channel = AstralRustString::new(found.into_bytes());
    }
    if let Some(operation) = unsafe { operation.as_mut() } {
        *operation = AstralRustString::new(kind.as_bytes().to_vec());
    }
    1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_transform_outgoing_message_body(
    host: *const c_char,
    host_length: usize,
    path: *const c_char,
    path_length: usize,
    method: *const c_char,
    method_length: usize,
    body: *const c_char,
    body_length: usize,
    transform: Option<Transform>,
    context: *mut c_void,
) -> AstralRustString {
    let Some(transform) = transform else { return optional(None) };
    optional(transform_body(
        text(host, host_length),
        text(path, path_length),
        text(method, method_length),
        bytes(body, body_length),
        transform,
        context,
    ))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_with_staff_user(body: *const c_char, length: usize) -> AstralRustString {
    optional(with_staff_user(bytes(body, length)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_with_staff_ready(message: *const c_char, length: usize) -> AstralRustString {
    optional(with_staff_ready(bytes(message, length)))
}
