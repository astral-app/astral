use std::ffi::CString;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

use super::state::BLOCKED_REQUESTS;
use crate::lua::host::http::{HttpRequest, HttpResponse};
use crate::network::policy as network_policy;

const IMAGE_URL_CAPACITY: usize = 1024;
const GET: &str = "GET";
const POST: &str = "POST";
const MESSAGES_SUFFIX: &str = "/messages";

static STAFF_USERS: AtomicU64 = AtomicU64::new(0);
static STAFF_READIES: AtomicU64 = AtomicU64::new(0);
static COMPRESSED_FRAMES: AtomicU64 = AtomicU64::new(0);
static DISCORD_REQUESTS: AtomicU64 = AtomicU64::new(0);
static MESSAGE_REQUESTS: AtomicU64 = AtomicU64::new(0);

pub struct Probe<'a> {
    pub method: &'a str,
    pub url: &'a str,
    pub host: &'a str,
    pub path: &'a str,
    pub headers: &'a str,
    pub json_body: bool,
}

#[derive(Default)]
pub struct Plan {
    pub block: bool,
    pub mark_staff: bool,
    pub transform_message: bool,
    pub intercept: bool,
    pub rewrite_image: bool,
}

impl Plan {
    pub fn handles(&self) -> bool {
        self.block || self.mark_staff || self.transform_message || self.intercept || self.rewrite_image
    }
}

fn method_or_get(method: &str) -> &str {
    if method.is_empty() { GET } else { method }
}

pub fn note(probe: &Probe) {
    if !network_policy::is_discord_host(probe.host) {
        return;
    }
    DISCORD_REQUESTS.fetch_add(1, Ordering::Relaxed);
    if probe.method == POST && probe.path.ends_with(MESSAGES_SUFFIX) {
        MESSAGE_REQUESTS.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn marks_staff(method: &str, host: &str, path: &str) -> bool {
    method_or_get(method) == GET && super::staff_dev_tools_enabled() && network_policy::is_current_user_request(host, path)
}

pub fn plan(probe: &Probe) -> Plan {
    let method = method_or_get(probe.method);
    let host = probe.host.to_ascii_lowercase();
    Plan {
        block: network_policy::is_telemetry_request(probe.host, probe.path),
        mark_staff: marks_staff(method, probe.host, probe.path),
        transform_message: probe.json_body
            && crate::lua::host::events::has_outgoing_message_transforms()
            && network_policy::outgoing_message_target(probe.host, probe.path, method).is_some(),
        intercept: crate::lua::host::http::wants_http(method, &host, probe.path),
        rewrite_image: rewrite_image(method, &host, probe.url).is_some(),
    }
}

pub fn rewrite_image(method: &str, host: &str, url: &str) -> Option<String> {
    if method_or_get(method) != GET || !network_policy::is_discord_host(&host.to_ascii_lowercase()) {
        return None;
    }
    let uri = CString::new(url).ok()?;
    let mut rewritten = vec![0u8; IMAGE_URL_CAPACITY];
    let changed =
        unsafe { crate::lua::host::nodes::astral_lua_image_rewrite(uri.as_ptr(), rewritten.as_mut_ptr().cast(), rewritten.len()) };
    if changed == 0 {
        return None;
    }
    let end = rewritten.iter().position(|byte| *byte == 0).unwrap_or(rewritten.len());
    let next = String::from_utf8_lossy(&rewritten[..end]).into_owned();
    (next != url).then_some(next)
}

pub fn transform_outgoing(method: &str, host: &str, path: &str, body: &[u8]) -> Option<Vec<u8>> {
    network_policy::transform_message_body(host, path, method, body, crate::lua::host::events::transform_outgoing_message)
}

fn headers_of(value: &Value) -> Vec<(String, String)> {
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

fn body_of(value: &Value) -> Vec<u8> {
    value
        .get("body_base64")
        .and_then(Value::as_str)
        .and_then(crate::lua::host::install::decode_base64)
        .unwrap_or_default()
}

fn request_of(value: &Value) -> HttpRequest {
    let text = |key: &str| value.get(key).and_then(Value::as_str).unwrap_or_default().to_owned();
    HttpRequest {
        method: method_or_get(&text("method")).to_owned(),
        url: text("url"),
        path: text("path"),
        headers: headers_of(value.get("headers").unwrap_or(&Value::Null)),
        body: body_of(value),
    }
}

fn response_of(value: &Value) -> HttpResponse {
    HttpResponse {
        status: value.get("status").and_then(Value::as_i64).unwrap_or(0),
        headers: headers_of(value.get("headers").unwrap_or(&Value::Null)),
        body: body_of(value),
    }
}

fn request_json(request: &HttpRequest) -> Value {
    json!({
        "method": request.method,
        "url": request.url,
        "path": request.path,
        "headers": headers_json(&request.headers),
        "body_base64": crate::lua::host::install::encode_base64(&request.body),
    })
}

fn response_json(response: &HttpResponse) -> Value {
    json!({
        "status": response.status,
        "headers": headers_json(&response.headers),
        "body_base64": crate::lua::host::install::encode_base64(&response.body),
    })
}

pub fn intercept_request(request: &str) -> Value {
    let parsed: Value = serde_json::from_str(request).unwrap_or(Value::Null);
    let mut plain = request_of(&parsed);
    let decision = super::intercept_http_request(&mut plain);
    if let Some(response) = decision.response {
        return json!({ "respond": response_json(&response) });
    }
    if decision.changed {
        return json!({ "request": request_json(&plain) });
    }
    Value::Null
}

pub fn intercept_response(request: &str, response: &str) -> Option<Value> {
    let request = request_of(&serde_json::from_str(request).unwrap_or(Value::Null));
    let mut plain = response_of(&serde_json::from_str(response).unwrap_or(Value::Null));
    super::intercept_http_response(&request, &mut plain).then(|| response_json(&plain))
}

pub fn mark_staff_user(body: &[u8]) -> Option<Vec<u8>> {
    let marked = network_policy::with_staff_user(body)?;
    STAFF_USERS.fetch_add(1, Ordering::Relaxed);
    Some(marked)
}

pub fn mark_staff_ready(message: &str) -> Option<String> {
    let marked = super::with_staff_ready(message)?;
    STAFF_READIES.fetch_add(1, Ordering::Relaxed);
    super::host::log("network filter: marked the account as staff in READY");
    Some(marked)
}

pub fn note_compressed_frame() {
    COMPRESSED_FRAMES.fetch_add(1, Ordering::Relaxed);
}

pub fn wants_uncompressed_gateway() -> bool {
    super::staff_dev_tools_enabled() || super::needs_gateway_events()
}

pub fn stats() -> Value {
    json!({
        "blocked": BLOCKED_REQUESTS.load(Ordering::Relaxed),
        "staffUsers": STAFF_USERS.load(Ordering::Relaxed),
        "staffReadies": STAFF_READIES.load(Ordering::Relaxed),
        "compressedFrames": COMPRESSED_FRAMES.load(Ordering::Relaxed),
        "hostCallsRewritten": crate::wire::host_calls::astral_host_calls_rewritten(),
        "discordRequests": DISCORD_REQUESTS.load(Ordering::Relaxed),
        "messageRequests": MESSAGE_REQUESTS.load(Ordering::Relaxed),
    })
}

const MEDIA_SAVE_WAIT: std::time::Duration = std::time::Duration::from_secs(12);

fn without_query(url: &str) -> &str {
    url.split(['?', '#']).next().unwrap_or(url)
}

pub fn has_media_save_handlers() -> bool {
    crate::lua::host::events::has_media_save_handlers()
}

pub fn replace_saved_media(url: &str, kind: crate::lua::enums::MediaKind) -> Option<String> {
    let (channel_id, message_id) = crate::discord::cache::message_containing(without_query(url)).unwrap_or_default();
    let (sender, receiver) = std::sync::mpsc::channel();
    let sender = std::sync::Mutex::new(sender);
    let request = crate::lua::host::events::resolve_media_save(
        crate::lua::host::events::MediaSave {
            url: url.to_owned(),
            kind,
            channel_id,
            message_id,
        },
        std::sync::Arc::new(move |replacement| {
            let _ = sender.lock().map(|sender| sender.send(replacement));
        }),
    );
    receiver.recv_timeout(MEDIA_SAVE_WAIT).unwrap_or_else(|_| {
        crate::lua::host::events::cancel_media_save(request);
        None
    })
}
