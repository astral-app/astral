use core::ffi::c_char;
use std::collections::{BTreeMap, VecDeque};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Once, OnceLock, RwLock};
use std::time::Duration;

use serde_json::{Value, json};

use crate::bridge::{AstralRustString, text};
use crate::host_callback::AstralCallback;
use crate::settings::{AstralSettingsHandle, store};
use crate::settings_pages::HostCall;

const PORT: u16 = 9324;
const RELISTEN_DELAY: Duration = Duration::from_millis(500);
const MAX_ASSET_BYTES: usize = 384 << 10;
const ASSET_EXTENSIONS: [&str; 7] = ["mp3", "m4a", "wav", "caf", "aac", "png", "webp"];
const MAX_LOG: usize = 500;
const MAX_REQUEST: usize = 512 << 10;
const PAIRING_ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

struct Hooks {
    host: Option<HostCall>,
    settings: usize,
    version: String,
    state_directory: String,
}

static HOOKS: RwLock<Hooks> = RwLock::new(Hooks {
    host: None,
    settings: 0,
    version: String::new(),
    state_directory: String::new(),
});
static LOG: Mutex<(VecDeque<Value>, u64)> = Mutex::new((VecDeque::new(), 0));
static STARTED: Once = Once::new();
static PAIRING: OnceLock<String> = OnceLock::new();

pub struct Request {
    pub method: String,
    pub route: String,
    pub query: BTreeMap<String, String>,
    pub body: String,
    pub json: Value,
}

pub struct Response {
    pub status: u16,
    pub body: Value,
}

fn response(status: u16, body: Value) -> Response {
    Response { status, body }
}

fn error(status: u16, message: &str) -> Response {
    response(status, json!({"error": message}))
}

fn hooks<T>(read: impl FnOnce(&Hooks) -> T) -> T {
    read(&HOOKS.read().unwrap_or_else(|poisoned| poisoned.into_inner()))
}

fn call_host(method: &str, arguments: Value, callback: AstralCallback) -> Value {
    let Some(host) = hooks(|hooks| hooks.host) else {
        drop(crate::host_callback::adopt_reply(callback));
        return Value::Null;
    };
    let arguments = arguments.to_string();
    let result = unsafe {
        host(
            method.as_ptr().cast(),
            method.len(),
            arguments.as_ptr().cast(),
            arguments.len(),
            callback,
        )
    };
    if result.bytes.is_null() {
        return Value::Null;
    }
    serde_json::from_slice(&crate::bridge::take(result)).unwrap_or(Value::Null)
}

fn host(method: &str, arguments: Value) -> Value {
    call_host(method, arguments, AstralCallback::none())
}

fn on_main(work: impl Fn() + Send + Sync + 'static) {
    call_host("onMain", json!([]), AstralCallback::from_changed(Arc::new(work)));
}

fn changed() {
    host("changed", json!([]));
}

fn plugins() -> Vec<Value> {
    match host("plugins", json!([])) {
        Value::Array(list) => list,
        _ => Vec::new(),
    }
}

fn plugin_named(identifier: &str) -> Option<Value> {
    plugins()
        .into_iter()
        .find(|plugin| plugin.get("id").and_then(Value::as_str) == Some(identifier))
}

fn field<'a>(object: &'a Value, key: &str) -> &'a str {
    object.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn decode_base64(text: &str) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    let (mut buffer, mut bits) = (0u32, 0);
    for character in text.bytes() {
        if character == b'=' {
            break;
        }
        let value = match character {
            b'A'..=b'Z' => character - b'A',
            b'a'..=b'z' => character - b'a' + 26,
            b'0'..=b'9' => character - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        buffer = (buffer << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            bytes.push((buffer >> bits) as u8);
        }
    }
    Some(bytes)
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let Ok(byte) = u8::from_str_radix(&value[index + 1..index + 3], 16)
        {
            decoded.push(byte);
            index += 3;
            continue;
        }
        decoded.push(if bytes[index] == b'+' { b' ' } else { bytes[index] });
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn split_path(path: &str, request: &mut Request) {
    let (route, query) = path.split_once('?').map_or((path, None), |(route, query)| (route, Some(query)));
    request.route = route.to_owned();
    for pair in query.into_iter().flat_map(|query| query.split('&')).filter(|pair| !pair.is_empty()) {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        request.query.insert(percent_decode(name), percent_decode(value));
    }
}

fn pairing_matches(offered: &str) -> bool {
    let expected = pairing_code();
    let given = offered.to_ascii_uppercase();
    if given.len() != expected.len() {
        return false;
    }
    expected
        .bytes()
        .zip(given.bytes())
        .fold(0u8, |difference, (left, right)| difference | (left ^ right))
        == 0
}

fn safe_relative(path: &str) -> bool {
    !path.is_empty() && !path.starts_with('/') && !path.contains("..") && !path.contains('\\')
}

fn write_file(path: &Path, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, bytes);
}

fn extension(path: &str) -> &str {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
}

fn put_plugin(request: &Request) -> Response {
    let body = &request.json;
    let (Some(manifest_text), Some(main_source)) = (
        body.get("manifest").and_then(Value::as_str),
        body.get("main").and_then(Value::as_str),
    ) else {
        return error(400, "manifest and main are required");
    };
    let Ok(manifest) = manifest_text.parse::<toml::Table>() else {
        return error(400, "invalid manifest");
    };
    let identifier = manifest.get("id").and_then(toml::Value::as_str).unwrap_or_default().to_owned();
    let main = manifest.get("main").and_then(toml::Value::as_str).unwrap_or("main.lua").to_owned();
    if identifier.is_empty() || identifier.contains('/') || identifier.contains("..") || main.contains('/') {
        return error(400, "invalid manifest");
    }
    let directory = PathBuf::from(hooks(|hooks| hooks.state_directory.clone()))
        .join("plugins")
        .join(&identifier);
    write_file(&directory.join("manifest.toml"), manifest_text.as_bytes());
    write_file(&directory.join(&main), main_source.as_bytes());
    for (path, module) in body.get("modules").and_then(Value::as_object).into_iter().flatten() {
        let translation = path.starts_with("i18n/") && extension(path) == "toml";
        if let Some(source) = module
            .as_str()
            .filter(|_| (extension(path) == "lua" || translation) && safe_relative(path))
        {
            write_file(&directory.join(path), source.as_bytes());
        }
    }
    for (path, asset) in body.get("assets").and_then(Value::as_object).into_iter().flatten() {
        let Some(encoded) = asset
            .as_str()
            .filter(|_| ASSET_EXTENSIONS.contains(&extension(path)) && safe_relative(path))
        else {
            continue;
        };
        if let Some(bytes) = decode_base64(encoded).filter(|bytes| bytes.len() <= MAX_ASSET_BYTES) {
            write_file(&directory.join(path), &bytes);
        }
    }
    let _ = std::fs::remove_file(directory.join("signature"));
    let reloaded = identifier.clone();
    on_main(move || {
        host("reload", json!([reloaded]));
        changed();
    });
    match plugin_named(&identifier) {
        Some(plugin) => response(
            200,
            json!({"id": identifier, "running": plugin.get("running").and_then(Value::as_bool).unwrap_or(false), "error": field(&plugin, "error")}),
        ),
        None => error(500, "the plugin did not load"),
    }
}

fn logs(request: &Request) -> Response {
    let since: u64 = request.query.get("since").and_then(|since| since.parse().ok()).unwrap_or(0);
    let log = LOG.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let lines: Vec<Value> = log
        .0
        .iter()
        .filter(|line| line["id"].as_u64().unwrap_or(0) > since)
        .cloned()
        .collect();
    response(200, Value::Array(lines))
}

fn status() -> Response {
    let mut body = host("status", json!([]));
    if !body.is_object() {
        body = json!({});
    }
    body["astral"] = json!(hooks(|hooks| hooks.version.clone()));
    body["plugins"] = Value::Array(plugins());
    response(200, body)
}

fn eval(request: &Request) -> Response {
    let body = &request.json;
    let (Some(plugin), Some(code)) = (body.get("plugin").and_then(Value::as_str), body.get("code").and_then(Value::as_str)) else {
        return error(400, "plugin and code are required");
    };
    response(
        200,
        json!({"result": host("eval", json!([plugin, code])).as_str().unwrap_or_default()}),
    )
}

fn set_plugin_enabled(request: &Request) -> Response {
    let body = &request.json;
    let (Some(identifier), Some(enabled)) = (body.get("id").and_then(Value::as_str), body.get("enabled").and_then(Value::as_bool)) else {
        return error(400, "id and enabled are required");
    };
    let target = identifier.to_owned();
    on_main(move || {
        host("setEnabled", json!([target, enabled]));
        changed();
    });
    match plugin_named(identifier) {
        Some(plugin) => response(
            200,
            json!({
                "id": identifier,
                "enabled": plugin.get("enabled").and_then(Value::as_bool).unwrap_or(false),
                "running": plugin.get("running").and_then(Value::as_bool).unwrap_or(false),
                "error": field(&plugin, "error"),
            }),
        ),
        None => error(404, "no such plugin"),
    }
}

fn platform_route(request: &Request) -> Option<Response> {
    let reply = host(
        "routes",
        json!({"method": request.method, "route": request.route, "query": request.query, "body": request.body}),
    );
    let status = reply.get("status").and_then(Value::as_u64)?;
    Some(response(status as u16, reply.get("body").cloned().unwrap_or_else(|| json!({}))))
}

pub fn handle(request: &mut Request, pairing: &str) -> Response {
    if !host("enabled", json!([])).as_bool().unwrap_or(false) {
        return error(403, "turn on developer mode in astral's settings");
    }
    if !pairing_matches(pairing) {
        return error(
            401,
            "pairing code missing or wrong; run astral dev pair <code> with the code under Developer mode",
        );
    }
    request.json = if request.body.is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&request.body).unwrap_or(Value::Null)
    };
    match (request.method.as_str(), request.route.as_str()) {
        ("GET", "/status") => status(),
        ("GET", "/logs") => logs(request),
        ("PUT", "/plugin") => put_plugin(request),
        ("POST", "/eval") => eval(request),
        ("POST", "/plugin/enabled") => set_plugin_enabled(request),
        _ => platform_route(request).unwrap_or_else(|| error(404, "unknown route")),
    }
}

fn serialize(response: &Response) -> Vec<u8> {
    let payload = response.body.to_string();
    format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
        response.status,
        if response.status == 200 { "OK" } else { "Error" },
        payload.len()
    )
    .into_bytes()
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

fn parse_head(head: &str, request: &mut Request) -> (usize, String) {
    let mut lines = head.split("\r\n");
    let first = lines.next().unwrap_or_default();
    if let Some((method, rest)) = first.split_once(' ') {
        request.method = method.to_owned();
        split_path(rest.split(' ').next().unwrap_or_default(), request);
    }
    let (mut length, mut pairing) = (0, String::new());
    for line in lines {
        let (name, value) = line
            .split_once(':')
            .map_or((line, ""), |(name, value)| (name, value.trim_matches([' ', '\t', '\r'])));
        match name.to_ascii_lowercase().as_str() {
            "content-length" => length = value.parse().unwrap_or(0),
            "x-astral-pairing" => pairing = value.to_owned(),
            _ => {}
        }
    }
    (length, pairing)
}

fn serve(mut client: TcpStream) {
    let mut received = Vec::new();
    let mut buffer = [0u8; 16384];
    let (mut expected, mut header_end) = (usize::MAX, 0);
    let mut request = Request {
        method: String::new(),
        route: String::new(),
        query: BTreeMap::new(),
        body: String::new(),
        json: Value::Null,
    };
    let mut pairing = String::new();
    while received.len() < MAX_REQUEST {
        let count = match client.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(count) => count,
        };
        received.extend_from_slice(&buffer[..count]);
        if header_end == 0 {
            let Some(end) = find(&received, b"\r\n\r\n") else { continue };
            header_end = end + 4;
            let (length, offered) = parse_head(&String::from_utf8_lossy(&received[..end]), &mut request);
            pairing = offered;
            expected = header_end + length;
        }
        if received.len() >= expected {
            break;
        }
    }
    if header_end != 0 && received.len() >= header_end {
        request.body = String::from_utf8_lossy(&received[header_end..]).into_owned();
    }
    let reply = if received.len() >= MAX_REQUEST {
        error(413, "request too large")
    } else {
        handle(&mut request, &pairing)
    };
    let _ = client.write_all(&serialize(&reply));
}

fn listen_loop() {
    loop {
        if let Ok(listener) = TcpListener::bind((Ipv4Addr::LOCALHOST, PORT)) {
            for client in listener.incoming() {
                match client {
                    Ok(client) => {
                        std::thread::spawn(move || serve(client));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
        }
        std::thread::sleep(RELISTEN_DELAY);
    }
}

pub fn append_log(line: &str) {
    let mut log = LOG.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    log.1 += 1;
    let entry = json!({"id": log.1, "time": crate::bridge::epoch_ms() as f64 / 1000.0, "line": line});
    log.0.push_back(entry);
    while log.0.len() > MAX_LOG {
        log.0.pop_front();
    }
}

fn random_code() -> String {
    let mut bytes = [0u8; 8];
    if let Ok(mut random) = std::fs::File::open("/dev/urandom") {
        let _ = random.read_exact(&mut bytes);
    }
    bytes
        .iter()
        .map(|byte| PAIRING_ALPHABET[usize::from(*byte) % PAIRING_ALPHABET.len()] as char)
        .collect()
}

pub fn pairing_code() -> String {
    PAIRING
        .get_or_init(|| {
            let handle = hooks(|hooks| hooks.settings) as *const AstralSettingsHandle;
            let stored = unsafe { store(handle) }
                .and_then(|mut settings| settings.get_string("astral", "dev-pairing-code"))
                .unwrap_or_default();
            let code = if stored.len() == 8 {
                stored
            } else {
                let code = random_code();
                if let Some(mut settings) = unsafe { store(handle) }
                    && settings.set("astral", "dev-pairing-code", json!(code))
                {
                    settings.save();
                }
                code
            };
            let file = PathBuf::from(hooks(|hooks| hooks.state_directory.clone())).join("dev-pairing-code");
            if std::fs::write(&file, &code).is_ok() {
                let _ = std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600));
            }
            code
        })
        .clone()
}

pub fn start(host: Option<HostCall>, settings: *const AstralSettingsHandle, version: String, state_directory: String) {
    STARTED.call_once(|| {
        *HOOKS.write().unwrap_or_else(|poisoned| poisoned.into_inner()) = Hooks {
            host,
            settings: settings as usize,
            version,
            state_directory,
        };
        pairing_code();
        std::thread::spawn(listen_loop);
    });
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_dev_start(
    host: Option<HostCall>,
    settings: *const AstralSettingsHandle,
    version: *const c_char,
    version_length: usize,
    state_directory: *const c_char,
    state_directory_length: usize,
) {
    start(
        host,
        settings,
        text(version, version_length),
        text(state_directory, state_directory_length),
    );
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_dev_append_log(line: *const c_char, length: usize) {
    append_log(&text(line, length));
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_dev_pairing_code() -> AstralRustString {
    AstralRustString::new(pairing_code().into_bytes())
}
