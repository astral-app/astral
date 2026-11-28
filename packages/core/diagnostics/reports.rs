use core::ffi::{c_char, c_int};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::{Map, Value, json};

use crate::diagnostics::privacy::sanitize;
use crate::support::bridge::{AstralRustString, bytes, path};
use crate::support::files::{read_text, write_atomically};

const MAX_REPORTS: usize = 20;
const MAX_ISSUE_BODY: usize = 6000;
const ISSUE_URL: &str = "https://github.com/astral-app/astral/issues/new";
const KINDS: [&str; 3] = ["plugin", "crash", "hang"];
const ENVIRONMENT_KEYS: [&str; 9] = [
    "device",
    "architecture",
    "abi",
    "system",
    "locale",
    "astral",
    "discord",
    "phoenix",
    "engine",
];

static FILE: Mutex<Option<PathBuf>> = Mutex::new(None);

fn clean(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(|text| sanitize(text.as_bytes()))
}

pub fn fingerprint(key: &str) -> String {
    let hash = key.bytes().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("{hash:016x}")
}

fn environment(source: Option<&Value>) -> Value {
    let source = source.filter(|value| value.is_object());
    let field = |key: &str| source.and_then(|source| source.get(key));
    let mut result = Map::new();
    result.insert("schema".into(), json!(2));
    result.insert(
        "platform".into(),
        json!(clean(field("platform")).unwrap_or_else(|| "unknown".to_owned())),
    );
    result.insert(
        "install_mode".into(),
        json!(clean(field("install_mode")).unwrap_or_else(|| "unknown".to_owned())),
    );
    for key in ENVIRONMENT_KEYS {
        if let Some(value) = clean(field(key)) {
            result.insert(key.into(), json!(value));
        }
    }
    if let Some(plugins) = field("plugins").and_then(Value::as_array) {
        result.insert(
            "plugins".into(),
            Value::Array(plugins.iter().filter_map(|plugin| clean(Some(plugin))).map(Value::String).collect()),
        );
    }
    Value::Object(result)
}

fn normalized(saved: &Value) -> Option<Value> {
    let kind = saved.get("kind").and_then(Value::as_str).unwrap_or("plugin");
    if !KINDS.contains(&kind) {
        return None;
    }
    let detail = clean(saved.get("detail").or_else(|| saved.get("error"))).unwrap_or_default();
    let title = clean(saved.get("title"))
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| kind.to_owned());
    let mut report = Map::new();
    report.insert("schema".into(), json!(2));
    report.insert("kind".into(), json!(kind));
    report.insert(
        "fingerprint".into(),
        json!(clean(saved.get("fingerprint")).unwrap_or_else(|| fingerprint(&format!("{title}{detail}")))),
    );
    report.insert("title".into(), json!(title));
    report.insert("detail".into(), json!(detail));
    for key in ["id", "plugin"] {
        if let Some(value) = clean(saved.get(key)) {
            report.insert(key.into(), json!(value));
        }
    }
    if let Some(time) = saved.get("time").filter(|time| time.is_number()) {
        report.insert("time".into(), time.clone());
    }
    if let Some(reason) = saved.get("reason").filter(|reason| reason.is_i64()) {
        report.insert("reason".into(), reason.clone());
    }
    report.insert("environment".into(), environment(saved.get("environment")));
    Some(Value::Object(report))
}

fn load(file: &Path) -> Vec<Value> {
    let saved: Value = read_text(file)
        .and_then(|data| serde_json::from_slice(&data).ok())
        .unwrap_or(Value::Null);
    let reports: Vec<Value> = saved
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(normalized)
        .take(MAX_REPORTS)
        .collect();
    if saved.as_array().is_some_and(|saved| *saved != reports) {
        write_atomically(file, Value::Array(reports.clone()).to_string().as_bytes());
    }
    reports
}

pub fn list() -> Vec<Value> {
    let guard = FILE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.as_deref().map(load).unwrap_or_default()
}

pub fn clear() -> bool {
    let guard = FILE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.as_ref().is_some_and(|file| write_atomically(file, b"[]"))
}

pub fn add(report: &Value) -> bool {
    let Some(report) = normalized(report) else { return false };
    let guard = FILE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(file) = guard.as_ref() else { return false };
    let mut reports = load(file);
    if reports
        .iter()
        .any(|existing| existing.get("fingerprint") == report.get("fingerprint"))
    {
        return false;
    }
    reports.insert(0, report);
    reports.truncate(MAX_REPORTS);
    write_atomically(file, Value::Array(reports).to_string().as_bytes())
}

fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |elapsed| elapsed.as_secs_f64())
}

pub fn plugin_failure(identifier: &str, name: &str, error: &str, environment: &Value) -> bool {
    let detail = sanitize(error.as_bytes());
    add(&json!({
        "kind": "plugin",
        "time": now(),
        "id": identifier,
        "plugin": identifier,
        "title": format!("{} stopped after errors", sanitize(name.as_bytes())),
        "detail": if detail.is_empty() { "Plugin failure".to_owned() } else { detail.clone() },
        "fingerprint": fingerprint(&format!("{identifier}{detail}")),
        "environment": environment,
    }))
}

pub fn unexpected_exit(identifier: &str, mut environment: Value, running: Value) -> bool {
    environment["plugins"] = running;
    add(&json!({
        "kind": "crash",
        "time": now(),
        "id": identifier,
        "plugin": identifier,
        "title": format!("Discord closed while {identifier} was running"),
        "detail": "Discord ended unexpectedly during a call into this plugin.",
        "fingerprint": fingerprint(&format!("exit:{identifier}")),
        "environment": environment,
    }))
}

pub fn process_exit(
    kind: &str,
    time: i64,
    reason: i64,
    detail: &str,
    mut environment: Value,
    active: &str,
    running: Option<Value>,
) -> bool {
    if (kind != "crash" && kind != "hang") || time <= 0 {
        return false;
    }
    let mut title = if kind == "hang" {
        "Discord stopped responding".to_owned()
    } else {
        "Discord process crashed".to_owned()
    };
    if let Some(running) = running {
        environment["plugins"] = running;
    }
    let mut report = json!({
        "kind": kind,
        "time": time,
        "reason": reason,
        "detail": if detail.trim().is_empty() { "No system diagnostic details were available." } else { detail },
        "fingerprint": fingerprint(&format!("exit:{kind}:{time}:{reason}")),
        "environment": environment,
    });
    if !active.is_empty() {
        report["plugin"] = json!(active);
        title.push_str(&if kind == "hang" {
            format!(" while {active} was running")
        } else {
            format!(" in {active}")
        });
    }
    report["title"] = json!(title);
    add(&report)
}

fn cell(value: Option<&Value>) -> String {
    value.and_then(Value::as_str).unwrap_or("").to_owned()
}

pub fn text(report: &Value) -> String {
    let environment = report.get("environment").cloned().unwrap_or(Value::Null);
    let field = |key: &str| cell(environment.get(key));
    let mut text = format!(
        "{}\n\n| | |\n| --- | --- |\n| Kind | {} |\n",
        cell(report.get("title")),
        cell(report.get("kind"))
    );
    for (label, key) in [
        ("astral", "astral"),
        ("Discord", "discord"),
        ("Engine", "engine"),
        ("Phoenix", "phoenix"),
        ("Device", "device"),
        ("Architecture", "architecture"),
        ("System", "system"),
        ("Locale", "locale"),
    ] {
        text.push_str(&format!("| {label} | {} |\n", field(key)));
    }
    if let Some(plugin) = report.get("plugin").and_then(Value::as_str) {
        text.push_str(&format!("| Plugin | {plugin} |\n"));
    }
    let running: Vec<&str> = environment
        .get("plugins")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    if !running.is_empty() {
        text.push_str(&format!("| Running plugins | {} |\n", running.join(", ")));
    }
    text.push_str(&format!("\n```text\n{}\n```\n", cell(report.get("detail"))));
    text
}

fn encode(component: &str) -> String {
    component
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

pub fn issue_url(report: &Value) -> String {
    let mut body = text(report);
    if body.chars().count() > MAX_ISSUE_BODY {
        body = body.chars().take(MAX_ISSUE_BODY).collect::<String>() + "\n```\n(truncated; the full report is on the clipboard)";
    }
    format!(
        "{ISSUE_URL}?title={}&body={}&labels={}",
        encode(&cell(report.get("title"))),
        encode(&body),
        encode(&cell(report.get("kind")))
    )
}

fn json_of(data: *const c_char, length: usize) -> Value {
    serde_json::from_slice(bytes(data, length)).unwrap_or(Value::Null)
}

fn string_of<'a>(data: *const c_char, length: usize) -> &'a str {
    std::str::from_utf8(bytes(data, length)).unwrap_or("")
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_reports_configure(directory: *const c_char, length: usize) {
    *FILE.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(path(directory, length).join("reports.json"));
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_reports_fingerprint(key: *const c_char, length: usize) -> AstralRustString {
    AstralRustString::new(fingerprint(string_of(key, length)).into_bytes())
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_reports_list() -> AstralRustString {
    AstralRustString::new(Value::Array(list()).to_string().into_bytes())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_reports_add(report: *const c_char, length: usize) -> c_int {
    c_int::from(add(&json_of(report, length)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_reports_plugin_failure(
    identifier: *const c_char,
    identifier_length: usize,
    name: *const c_char,
    name_length: usize,
    error: *const c_char,
    error_length: usize,
    environment: *const c_char,
    environment_length: usize,
) -> c_int {
    c_int::from(plugin_failure(
        string_of(identifier, identifier_length),
        string_of(name, name_length),
        string_of(error, error_length),
        &json_of(environment, environment_length),
    ))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_reports_unexpected_exit(
    identifier: *const c_char,
    identifier_length: usize,
    environment: *const c_char,
    environment_length: usize,
    running: *const c_char,
    running_length: usize,
) -> c_int {
    c_int::from(unexpected_exit(
        string_of(identifier, identifier_length),
        json_of(environment, environment_length),
        json_of(running, running_length),
    ))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_reports_process_exit(
    kind: *const c_char,
    kind_length: usize,
    time: i64,
    reason: i64,
    detail: *const c_char,
    detail_length: usize,
    environment: *const c_char,
    environment_length: usize,
    active: *const c_char,
    active_length: usize,
    running: *const c_char,
    running_length: usize,
) -> c_int {
    let running = json_of(running, running_length);
    c_int::from(process_exit(
        string_of(kind, kind_length),
        time,
        reason,
        &sanitize(bytes(detail, detail_length)),
        json_of(environment, environment_length),
        &sanitize(bytes(active, active_length)),
        running.is_array().then_some(running),
    ))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_reports_text(report: *const c_char, length: usize) -> AstralRustString {
    AstralRustString::new(text(&json_of(report, length)).into_bytes())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_reports_issue_url(report: *const c_char, length: usize) -> AstralRustString {
    AstralRustString::new(issue_url(&json_of(report, length)).into_bytes())
}
