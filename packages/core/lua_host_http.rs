use crate::capability::Capability;
use core::ffi::c_int;
use std::collections::BTreeMap;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use serde_json::{Value, json};

use crate::lua_api::{self as lua, LuaState, NOREF};
use crate::lua_host::{self as host, Plugin};
use crate::lua_json::{push_json, to_json};

pub const HOLD_REQUEST: &str = "hold";
pub const MAXIMUM_HTTP_BODY: usize = 4 << 20;
const MAXIMUM_HOLD: Duration = Duration::from_secs(600);
pub const COMMAND_APPLICATION: &str = "1000000000000000001";

#[derive(Clone, Default)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

#[derive(Clone, Default)]
pub struct HttpResponse {
    pub status: i64,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

#[derive(Default)]
pub struct HttpDecision {
    pub changed: bool,
    pub response: Option<HttpResponse>,
}

#[derive(Default)]
struct HoldState {
    done: bool,
    response: Option<HttpResponse>,
    changes: Option<Value>,
}

struct Hold {
    owner: usize,
    state: Mutex<HoldState>,
    ready: Condvar,
}

static HOLDS: Mutex<BTreeMap<u64, Arc<Hold>>> = Mutex::new(BTreeMap::new());

fn holds() -> std::sync::MutexGuard<'static, BTreeMap<u64, Arc<Hold>>> {
    HOLDS.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn hidden_header(name: &str) -> bool {
    matches!(name, "authorization" | "cookie" | "set-cookie")
}

pub fn contains_control(value: &str) -> bool {
    value.bytes().any(|byte| byte <= 0x20 || byte == 0x7f)
}

pub fn finish_hold(exchange: u64, response: Option<HttpResponse>, changes: Option<Value>) {
    let Some(hold) = holds().remove(&exchange) else { return };
    let mut state = hold.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    state.response = response;
    state.changes = changes;
    state.done = true;
    hold.ready.notify_all();
}

pub(crate) fn release_hooks(plugin: *mut Plugin) {
    let plugin_ref = unsafe { &mut *plugin };
    let state = plugin_ref.state();
    for hook in core::mem::take(&mut plugin_ref.http_hooks) {
        unsafe {
            lua::release(state, hook.request);
            lua::release(state, hook.response);
        }
    }
    let owned: Vec<u64> = holds()
        .iter()
        .filter(|(_, hold)| hold.owner == plugin as usize)
        .map(|(exchange, _)| *exchange)
        .collect();
    for exchange in owned {
        finish_hold(exchange, None, None);
    }
}

fn pattern_matches(state: *mut LuaState, text: &str, pattern: &str) -> bool {
    unsafe {
        let top = lua::lua_gettop(state);
        lua::lua_getglobal(state, c"string".as_ptr());
        if !lua::is_table(state, -1) {
            lua::lua_settop(state, top);
            return false;
        }
        lua::get_field(state, -1, "find");
        lua::push_str(state, text);
        lua::push_str(state, pattern);
        let matched = lua::pcall(state, 2, 1) == lua::OK && lua::lua_type(state, -1) != lua::TNIL;
        lua::lua_settop(state, top);
        matched
    }
}

fn hook_matches(plugin: *mut Plugin, hook: &crate::lua_host::HttpHook, method: &str, path: &str) -> bool {
    if !hook.method.is_empty() && hook.method != method {
        return false;
    }
    pattern_matches(unsafe { (*plugin).state() }, path, &hook.pattern)
}

unsafe fn push_headers(state: *mut LuaState, headers: &[(String, String)]) {
    unsafe { lua::lua_createtable(state, 0, headers.len() as c_int) };
    for (name, value) in headers.iter().filter(|(name, _)| !hidden_header(name)) {
        unsafe {
            lua::get_field(state, -1, name);
            let joined = match lua::to_bytes(state, -1).filter(|_| lua::lua_isstring(state, -1) != 0) {
                Some(previous) => format!("{}, {value}", String::from_utf8_lossy(previous)),
                None => value.clone(),
            };
            lua::pop(state, 1);
            lua::push_str(state, &joined);
            lua::set_field(state, -2, name);
        }
    }
}

unsafe fn push_request(state: *mut LuaState, exchange: u64, request: &HttpRequest) {
    unsafe {
        lua::lua_createtable(state, 0, 6);
        lua::push_str(state, &exchange.to_string());
        lua::set_field(state, -2, "id");
        lua::push_str(state, &request.method);
        lua::set_field(state, -2, "method");
        lua::push_str(state, &request.url);
        lua::set_field(state, -2, "url");
        lua::push_str(state, &request.path);
        lua::set_field(state, -2, "path");
        push_headers(state, &request.headers);
        lua::set_field(state, -2, "headers");
        lua::push_bytes(state, &request.body);
        lua::set_field(state, -2, "body");
    }
}

fn headers_from_json(value: &Value, previous: &[(String, String)]) -> Vec<(String, String)> {
    let mut headers: Vec<(String, String)> = previous.iter().filter(|(name, _)| hidden_header(name)).cloned().collect();
    for (name, header) in value.as_object().into_iter().flatten() {
        let lower = name.to_ascii_lowercase();
        let Some(text) = header.as_str() else { continue };
        if hidden_header(&lower) || contains_control(&lower) || contains_control(text) {
            continue;
        }
        headers.push((lower, text.to_owned()));
    }
    headers
}

pub fn url_host(url: &str) -> String {
    let Some(start) = url.find("://").map(|start| start + 3) else {
        return String::new();
    };
    let rest = &url[start..];
    let end = rest.find(['/', '?', '#', ':']).unwrap_or(rest.len());
    rest[..end].to_ascii_lowercase()
}

pub fn url_path(url: &str) -> String {
    let start = match url.find("://") {
        Some(scheme) => match url[scheme + 3..].find('/') {
            Some(slash) => scheme + 3 + slash,
            None => return "/".into(),
        },
        None => 0,
    };
    let rest = &url[start..];
    rest[..rest.find(['?', '#']).unwrap_or(rest.len())].to_owned()
}

fn apply_request_changes(request: &mut HttpRequest, changes: &Value) {
    if let Some(method) = changes.get("method").and_then(Value::as_str) {
        request.method = method.to_owned();
    }
    if let Some(url) = changes.get("url").and_then(Value::as_str)
        && url.starts_with("https://")
        && crate::network_policy::is_discord_host(&url_host(url))
    {
        request.url = url.to_owned();
        request.path = url_path(url);
    }
    if let Some(headers) = changes.get("headers").filter(|headers| headers.is_object()) {
        request.headers = headers_from_json(headers, &request.headers);
    }
    if let Some(body) = changes
        .get("body")
        .and_then(Value::as_str)
        .filter(|body| body.len() <= MAXIMUM_HTTP_BODY)
    {
        request.body = body.as_bytes().to_vec();
    }
}

fn response_from_json(value: &Value) -> HttpResponse {
    let mut status = value.get("status").and_then(Value::as_i64).unwrap_or(200);
    if !(100..=599).contains(&status) {
        status = 200;
    }
    HttpResponse {
        status,
        headers: value
            .get("headers")
            .filter(|headers| headers.is_object())
            .map(|headers| headers_from_json(headers, &[]))
            .unwrap_or_default(),
        body: value
            .get("body")
            .and_then(Value::as_str)
            .map(|body| body.as_bytes().to_vec())
            .unwrap_or_default(),
    }
}

pub unsafe fn exchange_argument(state: *mut LuaState, index: c_int) -> Result<u64, String> {
    let text = unsafe { crate::lua_host_api::check_text(state, index) }?;
    match text.parse::<u64>() {
        Ok(parsed) if parsed != 0 => Ok(parsed),
        _ => Err(format!("astral.net: '{text}' is not a held request")),
    }
}

pub unsafe fn register_intercept(state: *mut LuaState) -> crate::lua_host_api::Outcome {
    let plugin = host::plugin_of(state);
    unsafe { crate::lua_host_api::require_permission(state, Capability::NetDiscord) }?;
    unsafe { crate::lua_host_api::check_type(state, 1, lua::TTABLE) }?;
    if unsafe { (*plugin).http_hooks.len() } >= 16 {
        return Err("astral.net.intercept: at most 16 intercepts per plugin".into());
    }
    unsafe { lua::get_field(state, 1, "path") };
    if unsafe { lua::lua_isstring(state, -1) } == 0 {
        return Err("astral.net.intercept: path must be a Lua pattern".into());
    }
    let pattern = String::from_utf8_lossy(unsafe { lua::to_bytes(state, -1) }.unwrap_or_default()).into_owned();
    unsafe { lua::get_field(state, 1, "method") };
    let method = if unsafe { lua::lua_isstring(state, -1) } != 0 {
        String::from_utf8_lossy(unsafe { lua::to_bytes(state, -1) }.unwrap_or_default()).into_owned()
    } else {
        String::new()
    };
    unsafe { lua::pop(state, 2) };
    let reference_of = |field: &str| unsafe {
        lua::get_field(state, 1, field);
        if lua::is_function(state, -1) {
            lua::reference(state)
        } else {
            lua::pop(state, 1);
            NOREF
        }
    };
    let request = reference_of("request");
    let response = reference_of("response");
    if request == NOREF && response == NOREF {
        return Err("astral.net.intercept: give a request or response function".into());
    }
    unsafe {
        (*plugin).http_hooks.push(crate::lua_host::HttpHook {
            method,
            pattern,
            request,
            response,
        })
    };
    Ok(0)
}

pub unsafe fn release_held(state: *mut LuaState) -> crate::lua_host_api::Outcome {
    unsafe { crate::lua_host_api::require_permission(state, Capability::NetDiscord) }?;
    let exchange = unsafe { exchange_argument(state, 1) }?;
    let changes = if unsafe { lua::is_table(state, 2) } {
        unsafe { to_json(state, 2, 0) }.filter(Value::is_object)
    } else {
        None
    };
    finish_hold(exchange, None, changes);
    Ok(0)
}

pub unsafe fn respond_held(state: *mut LuaState) -> crate::lua_host_api::Outcome {
    unsafe { crate::lua_host_api::require_permission(state, Capability::NetDiscord) }?;
    let exchange = unsafe { exchange_argument(state, 1) }?;
    let Some(value) = (unsafe { to_json(state, 2, 0) }).filter(Value::is_object) else {
        return Err("astral.net.respond: give a response table".into());
    };
    finish_hold(exchange, Some(response_from_json(&value)), None);
    Ok(0)
}

fn command_id(plugin: &str, name: &str) -> String {
    let mut hash: u64 = 1_469_598_103_934_665_603;
    for byte in format!("{plugin}/{name}").bytes() {
        hash = (hash ^ u64::from(byte)).wrapping_mul(1_099_511_628_211);
    }
    ((hash >> 2) | (1 << 60)).to_string()
}

fn has_commands() -> bool {
    host::plugins()
        .into_iter()
        .any(|plugin| unsafe { (*plugin).running() && !(*plugin).commands.is_empty() })
}

fn is_command_index(method: &str, path: &str) -> bool {
    const SUFFIX: &str = "/application-command-index";
    method == "GET" && path.len() > SUFFIX.len() && path.ends_with(SUFFIX)
}

fn is_interaction(method: &str, path: &str) -> bool {
    method == "POST" && path.ends_with("/interactions")
}

fn inject_commands(body: &mut Vec<u8>) -> bool {
    let Ok(mut index) = serde_json::from_slice::<Value>(body) else {
        return false;
    };
    if !index.is_object() {
        return false;
    }
    if !index.get("applications").is_some_and(Value::is_array) {
        index["applications"] = json!([]);
    }
    if !index.get("application_commands").is_some_and(Value::is_array) {
        index["application_commands"] = json!([]);
    }
    if let Some(applications) = index["applications"].as_array_mut() {
        applications.push(json!({
            "id": COMMAND_APPLICATION, "name": "astral", "description": "Commands from astral plugins",
            "icon": null, "bot_id": COMMAND_APPLICATION, "flags": "0",
        }));
    }
    for plugin in host::plugins() {
        let plugin = unsafe { &*plugin };
        if !plugin.running() {
            continue;
        }
        for command in &plugin.commands {
            if let Some(commands) = index["application_commands"].as_array_mut() {
                commands.push(json!({
                    "id": command_id(plugin.identifier(), &command.name), "application_id": COMMAND_APPLICATION, "version": "1",
                    "type": 1, "name": command.name, "description": command.description, "options": command.options,
                    "contexts": [0, 1, 2], "integration_types": [0, 1], "dm_permission": true,
                }));
            }
        }
    }
    *body = index.to_string().into_bytes();
    true
}

fn find_bytes(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if from > haystack.len() {
        return None;
    }
    haystack[from..]
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|index| index + from)
}

fn interaction_payload(request: &HttpRequest) -> Option<Value> {
    let content_type = request
        .headers
        .iter()
        .filter(|(name, _)| name == "content-type")
        .map(|(_, value)| value.as_str())
        .next_back()
        .unwrap_or("");
    if !content_type.contains("multipart/form-data") {
        return serde_json::from_slice::<Value>(&request.body).ok().filter(Value::is_object);
    }
    let field = find_bytes(&request.body, b"name=\"payload_json\"", 0)?;
    let start = find_bytes(&request.body, b"\r\n\r\n", field)? + 4;
    let end = find_bytes(&request.body, b"\r\n--", start).unwrap_or(request.body.len());
    serde_json::from_slice::<Value>(&request.body[start..end])
        .ok()
        .filter(Value::is_object)
}

unsafe fn push_option_values(state: *mut LuaState, options: &Value) {
    unsafe { lua::lua_createtable(state, 0, 4) };
    for option in options.as_array().into_iter().flatten() {
        let Some(name) = option.get("name").and_then(Value::as_str) else {
            continue;
        };
        unsafe {
            push_json(state, option.get("value").unwrap_or(&Value::Null), 0);
            lua::set_field(state, -2, name);
        }
    }
}

fn multipart_message(payload: &Value, filename: &str, content_type: &str, data: &[u8]) -> (Vec<u8>, String) {
    let boundary = format!(
        "astral{}",
        std::time::Instant::now().elapsed().as_nanos() ^ u128::from(host::next_request())
    );
    let mut body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"payload_json\"\r\nContent-Type: application/json\r\n\r\n{payload}\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"files[0]\"; filename=\"{filename}\"\r\nContent-Type: {content_type}\r\n\r\n"
    )
    .into_bytes();
    body.extend_from_slice(data);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    (body, boundary)
}

fn dispatch_gateway(message: &Value) -> bool {
    host::platform_flag("dispatchGateway", &json!([message.to_string()]))
}

fn handle_interaction(request: &mut HttpRequest) -> Option<HttpDecision> {
    let payload = interaction_payload(request)?;
    if payload.get("application_id").and_then(Value::as_str) != Some(COMMAND_APPLICATION) {
        return None;
    }
    let data = payload.get("data").cloned().unwrap_or_else(|| json!({}));
    let name = data.get("name").and_then(Value::as_str).unwrap_or_default().to_owned();
    let channel = payload.get("channel_id").and_then(Value::as_str).unwrap_or_default().to_owned();
    let mut decision = HttpDecision {
        changed: false,
        response: Some(HttpResponse {
            status: 204,
            headers: Vec::new(),
            body: Vec::new(),
        }),
    };
    for plugin in host::plugins() {
        if !unsafe { (*plugin).running() } {
            continue;
        }
        let Some(reference) = (unsafe { &*plugin })
            .commands
            .iter()
            .find(|command| command.name == name)
            .map(|command| command.reference)
        else {
            continue;
        };
        let state = unsafe { (*plugin).state() };
        let top = unsafe { lua::lua_gettop(state) };
        unsafe {
            lua::push_reference(state, reference);
            push_option_values(state, data.get("options").unwrap_or(&json!([])));
            lua::lua_createtable(state, 0, 2);
            lua::push_str(state, &channel);
            lua::set_field(state, -2, "channel_id");
            if let Some(guild) = payload.get("guild_id").and_then(Value::as_str) {
                lua::push_str(state, guild);
                lua::set_field(state, -2, "guild_id");
            }
        }
        if !host::call(plugin, 2, 1, "command") {
            unsafe { lua::lua_settop(state, top) };
            host::fail_and_stop(plugin);
            return Some(decision);
        }
        let reply = if unsafe { lua::lua_type(state, -1) } != lua::TNIL {
            unsafe { to_json(state, -1, 0) }
        } else {
            None
        };
        unsafe { lua::lua_settop(state, top) };
        let nonce = payload.get("nonce").and_then(Value::as_str).unwrap_or_default().to_owned();
        let interaction = command_id(
            "interaction",
            &host::NEXT_REQUEST.fetch_add(1, std::sync::atomic::Ordering::Relaxed).to_string(),
        );
        if host::platform_has("dispatchGateway") {
            dispatch_gateway(&json!({"op": 0, "t": "INTERACTION_CREATE", "s": null, "d": {"id": interaction, "nonce": nonce}}));
            dispatch_gateway(&json!({"op": 0, "t": "INTERACTION_SUCCESS", "s": null, "d": {"id": interaction, "nonce": nonce}}));
        }
        let sends = reply.as_ref().is_some_and(|reply| reply.get("send").is_some_and(Value::is_object))
            && crate::lua_host_ui::snowflake_text(&channel);
        if (reply.is_none() || sends) && !nonce.is_empty() {
            dispatch_gateway(&json!({"op": 0, "t": "MESSAGE_DELETE", "s": null, "d": {"id": nonce, "channel_id": channel}}));
        }
        let Some(mut reply) = reply else { return Some(decision) };
        if reply.is_string() {
            reply = json!({"content": reply});
        }
        if sends {
            let mut message = reply["send"].clone();
            let file = message
                .as_object_mut()
                .and_then(|object| object.remove("file"))
                .unwrap_or(Value::Null);
            if !nonce.is_empty() {
                message["nonce"] = json!(nonce);
            }
            request.method = "POST".into();
            request.url = format!("https://discord.com/api/v9/channels/{channel}/messages");
            request.path = url_path(&request.url);
            let mut headers: Vec<(String, String)> = request
                .headers
                .iter()
                .filter(|(header, _)| header != "content-type" && header != "content-length")
                .cloned()
                .collect();
            if let Some(file_data) = file.get("data").and_then(Value::as_str) {
                let filename = file.get("name").and_then(Value::as_str).unwrap_or("file.png").to_owned();
                let mut attachment = json!({"id": 0, "filename": filename});
                if let Some(description) = file.get("description").and_then(Value::as_str) {
                    attachment["description"] = json!(description.chars().take(1024).collect::<String>());
                }
                message["attachments"] = json!([attachment]);
                let content_type = file.get("content_type").and_then(Value::as_str).unwrap_or("image/png");
                let (body, boundary) = multipart_message(&message, &filename, content_type, file_data.as_bytes());
                request.body = body;
                headers.push(("content-type".into(), format!("multipart/form-data; boundary={boundary}")));
            } else {
                request.body = message.to_string().into_bytes();
                headers.push(("content-type".into(), "application/json".into()));
            }
            request.headers = headers;
            decision.response = None;
            decision.changed = true;
            return Some(decision);
        }
        if reply.is_object() && crate::lua_host_ui::snowflake_text(&channel) {
            let mut local = reply.clone();
            if let Some(object) = local.as_object_mut() {
                object.remove("send");
            }
            local["nonce"] = json!(nonce);
            local["type"] = json!(20);
            local["application_id"] = json!(COMMAND_APPLICATION);
            let current = host::host().current_user_id.clone();
            let mut user = json!({"id": current});
            if !current.is_empty()
                && let Some(cached) = crate::discord_cache::user(&current).filter(Value::is_object)
            {
                user = cached;
            }
            local["interaction_metadata"] = json!({"id": interaction, "type": 2, "name": name, "user": user});
            crate::lua_host_ui::send_local_message_json(&channel, &local);
        }
        return Some(decision);
    }
    Some(decision)
}

pub fn wants_http(method: &str, host_name: &str, path: &str) -> bool {
    if !crate::network_policy::is_discord_host(host_name) {
        return false;
    }
    let _held = host::lock();
    if (is_command_index(method, path) || is_interaction(method, path)) && has_commands() {
        return true;
    }
    host::plugins()
        .into_iter()
        .any(|plugin| unsafe { (*plugin).running() && (*plugin).http_hooks.iter().any(|hook| hook_matches(plugin, hook, method, path)) })
}

pub fn intercept_request(request: &mut HttpRequest) -> HttpDecision {
    let mut decision = HttpDecision::default();
    if request.body.len() > MAXIMUM_HTTP_BODY {
        return decision;
    }
    let _held = host::lock();
    if is_interaction(&request.method, &request.path)
        && has_commands()
        && let Some(handled) = handle_interaction(request)
    {
        return handled;
    }
    for plugin in host::plugins() {
        let hooks = unsafe { (*plugin).http_hooks.len() };
        for index in 0..hooks {
            let (running, count) = unsafe { ((*plugin).running(), (*plugin).http_hooks.len()) };
            if !running || index >= count {
                break;
            }
            let (request_reference, method, pattern) = unsafe {
                let hook = &(&(*plugin).http_hooks)[index];
                (hook.request, hook.method.clone(), hook.pattern.clone())
            };
            let hook = crate::lua_host::HttpHook {
                method,
                pattern,
                request: request_reference,
                response: NOREF,
            };
            if request_reference == NOREF || !hook_matches(plugin, &hook, &request.method, &request.path) {
                continue;
            }
            let exchange = host::NEXT_REQUEST.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let state = unsafe { (*plugin).state() };
            unsafe {
                lua::push_reference(state, request_reference);
                push_request(state, exchange, request);
            }
            if !host::call(plugin, 1, 1, "net.intercept request") {
                host::fail_and_stop(plugin);
                break;
            }
            let hold = unsafe { lua::lua_type(state, -1) == lua::TSTRING && lua::to_bytes(state, -1) == Some(HOLD_REQUEST.as_bytes()) };
            let mut changes = if unsafe { lua::is_table(state, -1) } {
                unsafe { to_json(state, -1, 0) }.filter(Value::is_object)
            } else {
                None
            };
            unsafe { lua::pop(state, 1) };
            if hold {
                let pending = Arc::new(Hold {
                    owner: plugin as usize,
                    state: Mutex::new(HoldState::default()),
                    ready: Condvar::new(),
                });
                holds().insert(exchange, pending.clone());
                let outcome = host::while_unlocked(|| {
                    let guard = pending.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                    let (guard, timeout) = pending
                        .ready
                        .wait_timeout_while(guard, MAXIMUM_HOLD, |state| !state.done)
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    let timed_out = timeout.timed_out() && !guard.done;
                    drop(guard);
                    if timed_out {
                        finish_hold(exchange, None, None);
                    }
                    let guard = pending.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                    (guard.changes.clone(), guard.response.clone())
                });
                changes = outcome.0;
                if let Some(response) = outcome.1 {
                    decision.response = Some(response);
                    return decision;
                }
            }
            let Some(changes) = changes else { continue };
            if changes.get("status").is_some() {
                decision.response = Some(response_from_json(&changes));
                return decision;
            }
            apply_request_changes(request, &changes);
            decision.changed = true;
        }
    }
    decision
}

pub fn intercept_response(request: &HttpRequest, response: &mut HttpResponse) -> bool {
    if response.body.len() > MAXIMUM_HTTP_BODY {
        return false;
    }
    let _held = host::lock();
    let mut changed = false;
    if is_command_index(&request.method, &request.path) && response.status == 200 && has_commands() {
        changed = inject_commands(&mut response.body);
    }
    for plugin in host::plugins() {
        let mut index = 0;
        while index < unsafe { (*plugin).http_hooks.len() } {
            if !unsafe { (*plugin).running() } {
                break;
            }
            let (response_reference, method, pattern) = unsafe {
                let hook = &(&(*plugin).http_hooks)[index];
                (hook.response, hook.method.clone(), hook.pattern.clone())
            };
            index += 1;
            let hook = crate::lua_host::HttpHook {
                method,
                pattern,
                request: NOREF,
                response: response_reference,
            };
            if response_reference == NOREF || !hook_matches(plugin, &hook, &request.method, &request.path) {
                continue;
            }
            let state = unsafe { (*plugin).state() };
            unsafe {
                lua::push_reference(state, response_reference);
                push_request(state, 0, request);
                lua::lua_createtable(state, 0, 3);
                lua::lua_pushinteger(state, response.status);
                lua::set_field(state, -2, "status");
                push_headers(state, &response.headers);
                lua::set_field(state, -2, "headers");
                lua::push_bytes(state, &response.body);
                lua::set_field(state, -2, "body");
            }
            if !host::call(plugin, 2, 1, "net.intercept response") {
                host::fail_and_stop(plugin);
                break;
            }
            if unsafe { lua::is_table(state, -1) }
                && let Some(value) = unsafe { to_json(state, -1, 0) }.filter(Value::is_object)
            {
                if let Some(status) = value
                    .get("status")
                    .and_then(Value::as_i64)
                    .filter(|status| (100..=599).contains(status))
                {
                    response.status = status;
                }
                if let Some(headers) = value.get("headers").filter(|headers| headers.is_object()) {
                    response.headers = headers_from_json(headers, &response.headers);
                }
                if let Some(body) = value.get("body").and_then(Value::as_str) {
                    response.body = body.as_bytes().to_vec();
                }
                changed = true;
            }
            unsafe { lua::pop(state, 1) };
        }
    }
    changed
}
