use serde_json::{Value, json};

use super::ffi::{raw, take};
use super::host::{self, Request};
use super::state::{Platform, app};
use crate::diagnostics::env_report::{AndroidFacts, LocaleFacts};
use crate::diagnostics::privacy::sanitize;
use crate::lua::host as lua_host;
use crate::plugins::trace::AstralPreviousSession;
use crate::support::bridge::AstralRustString;

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
const ENVIRONMENT_SCHEMA: i64 = 2;
const CRASH_INTAKE_URL: &str = "https://astral-crash-intake.castdrian.workers.dev";
const INTAKE_TIMEOUT_MS: u64 = 15_000;

struct PreviousSession {
    started: i64,
    active: String,
    running: Value,
}

fn empty_session() -> AstralPreviousSession {
    AstralPreviousSession {
        started: 0,
        active: AstralRustString::new(Vec::new()),
        last: AstralRustString::new(Vec::new()),
        running: AstralRustString::new(Vec::new()),
    }
}

fn adopt(session: AstralPreviousSession) -> PreviousSession {
    let running = take(session.running);
    drop(take(session.last));
    PreviousSession {
        started: session.started,
        active: take(session.active),
        running: Value::Array(
            running
                .split('\0')
                .filter(|name| !name.is_empty())
                .map(|name| json!(name))
                .collect(),
        ),
    }
}

pub struct OpenedSession(Option<PreviousSession>);

pub fn open_trace(state: &str, now: i64) -> OpenedSession {
    let (directory, length) = raw(state);
    let mut session = empty_session();
    let found = unsafe { crate::plugins::trace::astral_trace_open(directory, length, now, &raw mut session) };
    let adopted = adopt(session);
    OpenedSession((found != 0).then_some(adopted))
}

pub fn record_unexpected_exit(opened: OpenedSession) {
    let Some(session) = opened.0.filter(|session| !session.active.is_empty()) else {
        return;
    };
    crate::diagnostics::reports::unexpected_exit(&session.active, report_environment(), session.running);
    send_pending();
}

fn previous_session() -> Option<PreviousSession> {
    let mut session = empty_session();
    let found = unsafe { crate::plugins::trace::astral_trace_previous(&raw mut session) };
    let adopted = adopt(session);
    (found != 0).then_some(adopted)
}

fn running_plugins() -> Vec<Value> {
    lua_host::plugins_json()
        .as_array()
        .into_iter()
        .flatten()
        .filter(|plugin| plugin.get("running").and_then(Value::as_bool) == Some(true))
        .map(|plugin| json!(sanitize(plugin.get("id").and_then(Value::as_str).unwrap_or_default().as_bytes())))
        .collect()
}

pub fn report_environment() -> Value {
    let Some(current) = app() else { return json!({}) };
    let stored = current.environment.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
    let mut environment = json!({});
    for key in ENVIRONMENT_KEYS.iter().copied().chain(["install_mode"]) {
        if let Some(value) = stored.get(key).and_then(Value::as_str) {
            environment[key] = json!(sanitize(value.as_bytes()));
        }
    }
    environment["schema"] = json!(ENVIRONMENT_SCHEMA);
    environment["platform"] = json!(current.platform.name());
    if environment.get("install_mode").is_none() {
        environment["install_mode"] = json!(current.platform.install_mode());
    }
    environment["plugins"] = Value::Array(running_plugins());
    environment
}

pub fn list() -> Value {
    Value::Array(crate::diagnostics::reports::list())
}

pub fn plugin_reports() -> Value {
    let plugins = lua_host::plugins_json();
    for plugin in plugins.as_array().into_iter().flatten() {
        if plugin.get("failed").and_then(Value::as_bool) != Some(true) {
            continue;
        }
        let identifier = plugin.get("id").and_then(Value::as_str).unwrap_or_default();
        let name = plugin.get("name").and_then(Value::as_str).unwrap_or(identifier);
        let error = plugin.get("error").and_then(Value::as_str).unwrap_or_default();
        crate::diagnostics::reports::plugin_failure(identifier, name, error, &report_environment());
    }
    list()
}

pub fn record_plugin_failure(identifier: &str, name: &str, error: &str) {
    host::log(&format!(
        "plugin {} failed: {}",
        sanitize(identifier.as_bytes()),
        sanitize(error.as_bytes())
    ));
    crate::diagnostics::reports::plugin_failure(identifier, name, error, &report_environment());
    send_pending();
}

pub fn record_process_exit(kind: &str, timestamp: i64, reason: i64, description: &str, trace: &str) {
    let mut detail = description.to_owned();
    if !trace.is_empty() {
        if !detail.is_empty() {
            detail.push('\n');
        }
        detail.push_str(trace);
    }
    let (active, running) = match previous_session() {
        Some(session) if session.started > 0 && timestamp >= session.started => (session.active, Some(session.running)),
        _ => (String::new(), None),
    };
    crate::diagnostics::reports::process_exit(kind, timestamp, reason, &detail, report_environment(), &active, running);
    send_pending();
}

fn report_text(report: &Value, key: &str) -> String {
    if let Some(value) = report.get(key).and_then(Value::as_str) {
        return value.to_owned();
    }
    if key == "detail" {
        return report.get("error").and_then(Value::as_str).unwrap_or_default().to_owned();
    }
    String::new()
}

fn intake_request(method: &str, path: &str, body: Option<String>) -> Option<Value> {
    let mut options = json!({
        "url": format!("{CRASH_INTAKE_URL}{path}"),
        "method": method,
        "timeout_ms": INTAKE_TIMEOUT_MS,
        "headers": { "content-type": "application/json" },
    });
    if let Some(body) = body {
        options["body"] = json!(body);
    }
    let response = crate::network::http::perform(&options);
    if response.get("ok").and_then(Value::as_bool) != Some(true) {
        host::log(&format!(
            "Failed to reach the crash intake at {path}: {}",
            response.get("status").and_then(Value::as_u64).unwrap_or(0)
        ));
        return None;
    }
    serde_json::from_str(response.get("body").and_then(Value::as_str).unwrap_or_default()).ok()
}

fn send(report: &Value) -> Option<Option<Value>> {
    let answer = intake_request("POST", "/v1/reports", Some(report.to_string()))?;
    let fingerprint = text(report, "fingerprint");
    let issue = answer.get("issue").filter(|issue| issue.is_object()).cloned();
    crate::diagnostics::reports::mark_sent(&fingerprint, issue.as_ref());
    Some(issue)
}

fn issue_url(report: &Value) -> Option<String> {
    report
        .get("issue")
        .and_then(|issue| issue.get("url"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn automatic_reports() -> bool {
    app().is_some_and(|current| current.flag("astral", "crash-reports", false))
}

pub fn send_pending() {
    if !automatic_reports() {
        return;
    }
    std::thread::spawn(|| {
        let unsent: Vec<Value> = list()
            .as_array()
            .into_iter()
            .flatten()
            .filter(|report| report.get("sent").and_then(Value::as_bool) != Some(true))
            .cloned()
            .collect();
        for report in &unsent {
            send(report);
        }
        if !unsent.is_empty() {
            super::refresh_settings();
        }
    });
}

pub fn refresh_issue_links() {
    std::thread::spawn(|| {
        let mut found = false;
        for report in list().as_array().into_iter().flatten() {
            if report.get("sent").and_then(Value::as_bool) != Some(true) || report.get("issue").is_some() {
                continue;
            }
            let fingerprint = text(report, "fingerprint");
            let Some(answer) = intake_request("GET", &format!("/v1/reports/{fingerprint}"), None) else {
                continue;
            };
            if let Some(issue) = answer.get("issue").filter(|issue| issue.is_object()) {
                found |= crate::diagnostics::reports::mark_sent(&fingerprint, Some(issue));
            }
        }
        if found {
            super::refresh_settings();
        }
    });
}

pub fn open_report(index: usize) {
    let reports = plugin_reports();
    let Some(report) = reports.as_array().and_then(|reports| reports.get(index)).cloned() else {
        return;
    };
    if let Some(url) = issue_url(&report) {
        host::request(Request::OpenUrl { url: &url });
        return;
    }
    std::thread::spawn(move || {
        match send(&report) {
            Some(Some(issue)) => {
                let url = issue.get("url").and_then(Value::as_str).unwrap_or_default().to_owned();
                host::request(Request::OpenUrl { url: &url });
            }
            Some(None) => host::toast(
                crate::support::translations::tr("ASTRAL_CRASH_REPORTED"),
                crate::support::translations::tr("ASTRAL_CRASH_REPORTED_SUBTITLE"),
            ),
            None => share_report(&report),
        }
        super::refresh_settings();
    });
}

fn share_report(report: &Value) {
    let mut content = report_text(report, "title");
    if content.is_empty() {
        content = "Plugin failure".to_owned();
    }
    content.push_str(&format!(" ({})\n\n{}", report_text(report, "id"), report_text(report, "detail")));
    if let Some(environment) = report.get("environment").and_then(Value::as_object) {
        for (key, value) in environment {
            let rendered = value.as_str().map_or_else(|| host::json_text(value), str::to_owned);
            content.push_str(&format!("\n{key}: {rendered}"));
        }
    }
    let url = crate::diagnostics::reports::issue_url(report);
    host::request(Request::OpenReport { text: &content, url: &url });
}

fn text(object: &Value, key: &str) -> String {
    object.get(key).and_then(Value::as_str).unwrap_or_default().to_owned()
}

fn flag(object: &Value, key: &str) -> bool {
    object.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn integer(object: &Value, key: &str) -> i64 {
    object.get(key).and_then(Value::as_i64).unwrap_or(0)
}

fn android_facts(device: &Value) -> AndroidFacts {
    let background = device.get("background").cloned().unwrap_or(Value::Null);
    let locale = device.get("locale").cloned().unwrap_or(Value::Null);
    AndroidFacts {
        selinux_enforcing: device.get("selinux").and_then(Value::as_bool),
        verified_boot_state: text(device, "verifiedBoot"),
        build_tags: text(device, "buildTags"),
        custom_rom: text(device, "rom"),
        page_size: integer(device, "pageSize"),
        web_view: text(device, "webView"),
        host: text(device, "host"),
        skin: text(device, "skin"),
        battery_optimized: flag(&background, "optimized"),
        background_restricted: flag(&background, "restricted"),
        standby_bucket: integer(&background, "bucket"),
        abi_bridge: text(device, "abiBridge"),
        low_ram: flag(device, "lowRam"),
        harmony_os: flag(device, "harmony"),
        adoptable_storage: flag(device, "adoptable"),
        refresh_rate: integer(device, "refreshRate"),
        bold_text: flag(device, "boldText"),
        form_factor: text(device, "formFactor"),
        duplicate_copy: String::new(),
        locale: LocaleFacts {
            calendar: text(&locale, "calendar"),
            non_latin_digits: flag(&locale, "nonLatinDigits"),
            right_to_left: flag(&locale, "rtl"),
        },
    }
}

fn device_report(platform: Platform) -> Value {
    let facts: Value = host::request(Request::EnvironmentFacts)
        .into_text()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null);
    let mut environment = json!({});
    if !facts.is_object() {
        return environment;
    }
    if platform == Platform::Ios {
        return crate::diagnostics::apple::report(&facts);
    }
    environment["install"] = json!(crate::diagnostics::env_report::android_install_line(
        facts.get("install").unwrap_or(&Value::Null)
    ));
    environment["framework"] = json!(text(&facts, "framework"));
    environment["device_name"] = json!(text(&facts, "device_name"));
    let device = facts.get("device").cloned().unwrap_or(Value::Null);
    let mut markers = crate::diagnostics::env_report::android_markers(&android_facts(&device));
    let stack = take(crate::network::stack::astral_current_network_stack_marker());
    if !stack.is_empty() {
        markers.push(stack);
    }
    environment["markers"] = json!(markers);
    environment
}

pub fn environment() -> Value {
    let Some(current) = app() else { return json!({}) };
    let mut environment = report_environment();
    if let (Some(target), Value::Object(device)) = (environment.as_object_mut(), device_report(current.platform)) {
        target.extend(device);
    }
    environment["commit"] = json!(current.commit);
    environment["image"] = json!(current.platform.image_name());
    let now = std::time::UNIX_EPOCH.elapsed().map_or(0.0, |elapsed| elapsed.as_secs_f64());
    if let (Some(target), Value::Object(summary)) = (
        environment.as_object_mut(),
        crate::diagnostics::env_report::crash_summary(&list(), now),
    ) {
        target.extend(summary);
    }
    environment
}

#[derive(Clone, Copy)]
enum DiagnosticKind {
    Crash,
    Hang,
}

impl DiagnosticKind {
    const ALL: [DiagnosticKind; 2] = [DiagnosticKind::Crash, DiagnosticKind::Hang];

    const fn payload_key(self) -> &'static str {
        match self {
            DiagnosticKind::Crash => "crashDiagnostics",
            DiagnosticKind::Hang => "hangDiagnostics",
        }
    }

    const fn name(self) -> &'static str {
        match self {
            DiagnosticKind::Crash => "crash",
            DiagnosticKind::Hang => "hang",
        }
    }

    const fn title(self) -> &'static str {
        match self {
            DiagnosticKind::Crash => "Crash",
            DiagnosticKind::Hang => "Hang",
        }
    }

    fn stack(self, diagnostic: &Value) -> Option<String> {
        match self {
            DiagnosticKind::Crash => crate::diagnostics::metrickit::crash_stack(diagnostic),
            DiagnosticKind::Hang => crate::diagnostics::metrickit::hang_stack(diagnostic),
        }
    }
}

pub fn record_diagnostics(payload: &Value) -> usize {
    let mut recorded = 0;
    for kind in DiagnosticKind::ALL {
        for diagnostic in payload.get(kind.payload_key()).and_then(Value::as_array).into_iter().flatten() {
            let Some(stack) = kind.stack(diagnostic) else { continue };
            let detail = sanitize(stack.as_bytes());
            let first_line = detail.lines().next().unwrap_or_default().to_owned();
            let now = std::time::UNIX_EPOCH.elapsed().map_or(0.0, |elapsed| elapsed.as_secs_f64());
            let added = crate::diagnostics::reports::add(&json!({
                "kind": kind.name(),
                "time": now,
                "title": format!("{}: {first_line}", kind.title()),
                "fingerprint": crate::diagnostics::reports::fingerprint(&detail),
                "detail": detail,
                "environment": report_environment(),
            }));
            recorded += usize::from(added);
        }
    }
    if recorded > 0 {
        send_pending();
    }
    recorded
}
