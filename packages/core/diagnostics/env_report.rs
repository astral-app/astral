use core::ffi::{c_char, c_int};
use std::sync::Mutex;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::support::bridge::{AstralRustString, bytes};

const STANDARD_PAGE_SIZE: i64 = 4096;

const PUBLIC_ROOTS: [&str; 22] = [
    "GTS Root",
    "Google Trust Services",
    "ISRG Root",
    "Let's Encrypt",
    "DigiCert",
    "Baltimore CyberTrust",
    "Sectigo",
    "USERTrust",
    "GlobalSign",
    "Amazon Root",
    "Amazon RSA",
    "Amazon ECDSA",
    "Starfield",
    "Go Daddy",
    "Entrust",
    "Comodo",
    "WE1",
    "WR1",
    "E5",
    "E6",
    "R10",
    "R11",
];
const USUAL_REFRESH_RATE: i64 = 60;
const OLD_WEBVIEW_MAJOR: i64 = 120;
const RARE_BUCKET: i64 = 40;
const RESTRICTED_BUCKET: i64 = 45;
const DAY: f64 = 24.0 * 60.0 * 60.0;

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LocaleFacts {
    pub calendar: String,
    pub non_latin_digits: bool,
    pub right_to_left: bool,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AppleFacts {
    pub model_edit: String,
    pub jailbreak_version: String,
    #[serde(rename = "iOSAppOnVision")]
    pub ios_app_on_vision: bool,
    #[serde(rename = "sideStoreVPN")]
    pub side_store_vpn: bool,
    pub live_container_multitask: bool,
    pub refresh_rate: i64,
    pub bold_text: bool,
    pub locale: LocaleFacts,
    pub lockdown_mode: bool,
    #[serde(rename = "iPhoneAppOnIPad")]
    pub iphone_app_on_ipad: bool,
    #[serde(rename = "iOSAppOnMac")]
    pub ios_app_on_mac: bool,
    pub play_cover: bool,
    pub managed_app: bool,
    pub jit_enabled: bool,
    pub debugger_attached: bool,
    pub troll_fools: bool,
    pub app_sync: bool,
    pub duplicate_copy: String,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AndroidFacts {
    pub selinux_enforcing: Option<bool>,
    pub verified_boot_state: String,
    pub build_tags: String,
    #[serde(rename = "customROM")]
    pub custom_rom: String,
    pub page_size: i64,
    pub web_view: String,
    pub host: String,
    pub skin: String,
    pub battery_optimized: bool,
    pub background_restricted: bool,
    pub standby_bucket: i64,
    pub abi_bridge: String,
    pub low_ram: bool,
    #[serde(rename = "harmonyOS")]
    pub harmony_os: bool,
    pub adoptable_storage: bool,
    pub refresh_rate: i64,
    pub bold_text: bool,
    pub form_factor: String,
    pub duplicate_copy: String,
    pub locale: LocaleFacts,
}

#[derive(Clone, Default, Deserialize)]
#[serde(default)]
pub struct HttpsCheck {
    pub completed: bool,
    pub issuers: Vec<String>,
    pub failure: String,
}

static LATEST_HTTPS: Mutex<Option<Value>> = Mutex::new(None);

pub fn is_beta_build(build: &str) -> bool {
    let digits = build.bytes().take_while(u8::is_ascii_digit).count();
    let bytes = build.as_bytes();
    if digits == 0 || digits + 1 >= bytes.len() || !bytes[digits].is_ascii_uppercase() {
        return false;
    }
    let rest = &bytes[digits + 1..];
    rest.len() >= 5 && rest[0] == b'5' && rest.last().is_some_and(u8::is_ascii_lowercase)
}

pub fn locale_markers(facts: &LocaleFacts) -> Vec<String> {
    let mut markers = Vec::new();
    if !facts.calendar.is_empty() && facts.calendar != "gregorian" {
        markers.push(format!("{} calendar", facts.calendar));
    }
    if facts.non_latin_digits {
        markers.push("non-Latin digits".to_owned());
    }
    if facts.right_to_left {
        markers.push("right-to-left language".to_owned());
    }
    markers
}

pub fn display_markers(refresh_rate: i64, bold_text: bool) -> Vec<String> {
    let mut markers = Vec::new();
    if refresh_rate > USUAL_REFRESH_RATE {
        markers.push(format!("{refresh_rate} Hz"));
    }
    if bold_text {
        markers.push("bold text".to_owned());
    }
    markers
}

pub fn duplicate_copy_marker(path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    format!("second Unbound copy skipped ({name})")
}

fn flag(markers: &mut Vec<String>, on: bool, text: &str) {
    if on {
        markers.push(text.to_owned());
    }
}

fn text(markers: &mut Vec<String>, value: &str) {
    if !value.is_empty() {
        markers.push(value.to_owned());
    }
}

pub fn apple_markers(facts: &AppleFacts) -> Vec<String> {
    let mut markers = Vec::new();
    if !facts.duplicate_copy.is_empty() {
        markers.push(duplicate_copy_marker(&facts.duplicate_copy));
    }
    text(&mut markers, &facts.model_edit);
    text(&mut markers, &facts.jailbreak_version);
    flag(&mut markers, facts.ios_app_on_vision, "iPhone app on Vision Pro");
    flag(&mut markers, facts.side_store_vpn, "SideStore pairing VPN");
    flag(&mut markers, facts.live_container_multitask, "LiveContainer multitasking");
    flag(&mut markers, facts.lockdown_mode, "Lockdown Mode");
    flag(&mut markers, facts.iphone_app_on_ipad, "iPhone app on iPad");
    if facts.play_cover {
        markers.push("PlayCover".to_owned());
    } else {
        flag(&mut markers, facts.ios_app_on_mac, "iPhone app on Mac");
    }
    flag(&mut markers, facts.managed_app, "MDM-managed");
    if facts.debugger_attached {
        markers.push("debugger attached".to_owned());
    } else {
        flag(&mut markers, facts.jit_enabled, "JIT enabled");
    }
    flag(&mut markers, facts.troll_fools, "TrollFools injection");
    flag(&mut markers, facts.app_sync, "AppSync Unified");
    markers.extend(display_markers(facts.refresh_rate, facts.bold_text));
    markers.extend(locale_markers(&facts.locale));
    markers
}

fn leading_number(text: &str) -> i64 {
    let digits: String = text.trim_start().chars().take_while(char::is_ascii_digit).collect();
    digits.parse().unwrap_or(0)
}

fn is_usual_web_view(web_view: &str) -> bool {
    ["com.google.android.webview ", "com.android.webview ", "com.android.chrome "]
        .iter()
        .find_map(|package| web_view.strip_prefix(package))
        .is_some_and(|version| leading_number(version) >= OLD_WEBVIEW_MAJOR)
}

fn background_limits(facts: &AndroidFacts) -> Vec<String> {
    let mut limits = Vec::new();
    flag(&mut limits, facts.background_restricted, "bg restricted");
    if facts.standby_bucket >= RESTRICTED_BUCKET {
        limits.push("standby restricted".to_owned());
    } else if facts.standby_bucket >= RARE_BUCKET {
        limits.push("standby rare".to_owned());
    }
    if !limits.is_empty() && facts.battery_optimized {
        limits.push("battery optimized".to_owned());
    }
    limits
}

pub fn android_markers(facts: &AndroidFacts) -> Vec<String> {
    let mut markers = Vec::new();
    if !facts.duplicate_copy.is_empty() {
        markers.push(duplicate_copy_marker(&facts.duplicate_copy));
    }
    flag(&mut markers, facts.selinux_enforcing == Some(false), "SELinux permissive");
    if !facts.verified_boot_state.is_empty() && facts.verified_boot_state != "green" {
        markers.push(format!("boot {}", facts.verified_boot_state));
    }
    if !facts.custom_rom.is_empty() {
        markers.push(facts.custom_rom.clone());
    } else if !facts.build_tags.is_empty() && facts.build_tags != "release-keys" {
        markers.push(facts.build_tags.clone());
    }
    if facts.page_size > 0 && facts.page_size != STANDARD_PAGE_SIZE {
        markers.push(format!("{} KB pages", facts.page_size / 1024));
    }
    if !facts.web_view.is_empty() && !is_usual_web_view(&facts.web_view) {
        markers.push(format!("WebView {}", facts.web_view));
    }
    text(&mut markers, &facts.host);
    let background = background_limits(facts);
    if !background.is_empty() {
        text(&mut markers, &facts.skin);
    }
    markers.extend(background);
    if !facts.abi_bridge.is_empty() && facts.abi_bridge != "0" {
        markers.push(format!("ABI translation ({})", facts.abi_bridge));
    }
    flag(&mut markers, facts.low_ram, "low-RAM device");
    flag(&mut markers, facts.harmony_os, "HarmonyOS");
    flag(&mut markers, facts.adoptable_storage, "installed on adoptable storage");
    text(&mut markers, &facts.form_factor);
    markers.extend(display_markers(facts.refresh_rate, facts.bold_text));
    markers.extend(locale_markers(&facts.locale));
    markers
}

fn clipped(text: &str, limit: usize) -> String {
    let mut end = text.len().min(limit);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

pub fn https_verdict(check: &HttpsCheck) -> Value {
    if !check.completed {
        return if check.failure.is_empty() {
            Value::Null
        } else {
            json!({ "intercepted": false, "failure": check.failure })
        };
    }
    if let Some(issuer) = check
        .issuers
        .iter()
        .find(|issuer| !PUBLIC_ROOTS.iter().any(|root| issuer.starts_with(root)))
    {
        return json!({ "intercepted": true, "issuer": clipped(issuer, 96) });
    }
    let mut verdict = json!({ "intercepted": false });
    if let Some(last) = check.issuers.last() {
        verdict["issuer"] = json!(clipped(last, 96));
    }
    verdict
}

pub fn android_install_line(facts: &Value) -> String {
    if !facts.is_object() {
        return String::new();
    }
    let field = |key: &str| facts.get(key).and_then(Value::as_str).unwrap_or("").to_owned();
    let mut parts = Vec::new();
    let signer = field("signer");
    if !signer.is_empty() {
        parts.push(format!("APK cert {}", clipped(&signer, 16)));
        parts.push(field("signerSubject"));
        if facts.get("discordSigned").and_then(Value::as_bool) != Some(true) {
            parts.push("re-signed".to_owned());
        }
    }
    parts.push(field("patch"));
    parts.push(field("framework"));
    let mut incomplete = false;
    for name in facts
        .get("missingSplits")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        parts.push(format!("missing {name}"));
        incomplete = true;
    }
    let splits = facts.get("splits").and_then(Value::as_i64).unwrap_or(0);
    if !incomplete && splits > 0 {
        parts.push(format!("{splits} splits"));
    }
    if facts.get("merged").and_then(Value::as_bool) == Some(true) {
        parts.push("merged split APK".to_owned());
    }
    parts.push(field("profile"));
    parts.into_iter().filter(|part| !part.is_empty()).collect::<Vec<_>>().join(" · ")
}

pub fn crash_summary(reports: &Value, now: f64) -> Value {
    let Some(list) = reports.as_array() else {
        return json!({ "reports": 0, "crashes_24h": 0 });
    };
    let mut summary = json!({ "reports": list.len(), "crashes_24h": 0 });
    if let Some(title) = list.first().and_then(|report| report.get("title")).and_then(Value::as_str) {
        summary["last_report"] = json!(title);
    }
    let recent = list
        .iter()
        .filter(|report| {
            let kind = report.get("kind").and_then(Value::as_str).unwrap_or("");
            let time = report.get("time").and_then(Value::as_f64).unwrap_or(0.0);
            (kind == "crash" || kind == "hang") && now - time <= DAY
        })
        .count();
    summary["crashes_24h"] = json!(recent);
    summary
}

fn parsed<T: for<'a> Deserialize<'a> + Default>(data: *const c_char, length: usize) -> T {
    serde_json::from_slice(bytes(data, length)).unwrap_or_default()
}

fn list(markers: Vec<String>) -> AstralRustString {
    AstralRustString::list(&markers.into_iter().map(String::into_bytes).collect::<Vec<_>>())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_env_is_beta_build(build: *const c_char, length: usize) -> c_int {
    c_int::from(is_beta_build(&String::from_utf8_lossy(bytes(build, length))))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_env_apple_markers(facts: *const c_char, length: usize) -> AstralRustString {
    list(apple_markers(&parsed(facts, length)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_env_android_markers(facts: *const c_char, length: usize) -> AstralRustString {
    list(android_markers(&parsed(facts, length)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_env_locale_markers(facts: *const c_char, length: usize) -> AstralRustString {
    list(locale_markers(&parsed(facts, length)))
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_env_display_markers(refresh_rate: i64, bold_text: c_int) -> AstralRustString {
    list(display_markers(refresh_rate, bold_text != 0))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_env_duplicate_copy_marker(path: *const c_char, length: usize) -> AstralRustString {
    AstralRustString::new(duplicate_copy_marker(&String::from_utf8_lossy(bytes(path, length))).into_bytes())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_env_https_verdict(check: *const c_char, length: usize) -> AstralRustString {
    AstralRustString::new(https_verdict(&parsed(check, length)).to_string().into_bytes())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_env_android_install_line(facts: *const c_char, length: usize) -> AstralRustString {
    let facts: Value = serde_json::from_slice(bytes(facts, length)).unwrap_or(Value::Null);
    AstralRustString::new(android_install_line(&facts).into_bytes())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_env_crash_summary(reports: *const c_char, length: usize, now: f64) -> AstralRustString {
    let reports: Value = serde_json::from_slice(bytes(reports, length)).unwrap_or(Value::Null);
    AstralRustString::new(crash_summary(&reports, now).to_string().into_bytes())
}

pub fn record_https_check(check: Value) {
    *LATEST_HTTPS.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(check);
}

pub fn current_https_check() -> Value {
    LATEST_HTTPS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
        .unwrap_or_else(|| json!({}))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_env_record_https_check(check: *const c_char, length: usize) {
    let check: Value = serde_json::from_slice(bytes(check, length)).unwrap_or(Value::Null);
    *LATEST_HTTPS.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(check);
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_env_current_https_check() -> AstralRustString {
    let current = LATEST_HTTPS.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
    AstralRustString::new(current.unwrap_or_else(|| json!({})).to_string().into_bytes())
}
