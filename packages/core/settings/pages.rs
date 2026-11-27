use crate::plugins::capability::Capability;
use core::ffi::{c_char, c_int};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use serde_json::{Value, json};

use crate::render::swatch::{color_text, parse_color, swatch_uri};
use crate::settings::store::{AstralSettingsHandle, store};
use crate::support::bridge::{AstralRustString, optional, text};
use crate::support::host_callback::{AstralCallback, Changed, Reply, adopt_changed};
use crate::support::translations::{tr, tr_with};
use crate::wire::decorate::astral_wire_stats;

const PLUGIN_ROUTE: &str = "plugin:";
const PLUGIN_SETTINGS_ROUTE: &str = "plugin-settings:";
const PLUGIN_COLOR_ROUTE: &str = "plugin-color:";
const CATALOG_REFRESH_SECONDS: i64 = 6 * 60 * 60;

const ROW_LINK: i64 = 0;
const ROW_SWITCH: i64 = 1;
const ROW_INFO: i64 = 2;
const ROW_SELECTED: i64 = 4;
const ROW_CHOICE: i64 = 5;
const ROW_NUMBER: i64 = 6;

pub type HostCall = unsafe extern "C" fn(
    method: *const c_char,
    method_length: usize,
    arguments: *const c_char,
    arguments_length: usize,
    callback: AstralCallback,
) -> AstralRustString;

struct Platform {
    host: Option<HostCall>,
    settings: usize,
    version: String,
}

static PLATFORM: RwLock<Platform> = RwLock::new(Platform {
    host: None,
    settings: 0,
    version: String::new(),
});
static FILTER: Mutex<String> = Mutex::new(String::new());

fn platform<T>(read: impl FnOnce(&Platform) -> T) -> T {
    read(&PLATFORM.read().unwrap_or_else(|poisoned| poisoned.into_inner()))
}

fn version() -> String {
    platform(|platform| platform.version.clone())
}

fn call_host(method: &str, arguments: &Value, callback: AstralCallback) -> Value {
    let Some(host) = platform(|platform| platform.host) else {
        drop(crate::support::host_callback::adopt_reply(callback));
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
    let text = crate::support::bridge::take(result);
    serde_json::from_slice(&text).unwrap_or(Value::Null)
}

fn host(method: &str, arguments: &Value) -> Value {
    call_host(method, arguments, AstralCallback::none())
}

fn host_with(method: &str, arguments: &Value, changed: Changed) -> Value {
    call_host(method, arguments, AstralCallback::from_changed(changed))
}

fn host_reply(method: &str, arguments: &Value, reply: Reply) -> Value {
    call_host(method, arguments, AstralCallback::from_reply(reply))
}

fn capable(name: &str) -> bool {
    host("capabilities", &json!([]))[name].as_bool().unwrap_or(false)
}

fn host_text(method: &str) -> String {
    host(method, &json!([])).as_str().unwrap_or_default().to_owned()
}

fn host_list(method: &str) -> Vec<Value> {
    match host(method, &json!([])) {
        Value::Array(list) => list,
        _ => Vec::new(),
    }
}

fn with_settings<T>(fallback: T, work: impl FnOnce(&mut crate::settings::store::Store) -> T) -> T {
    let handle = platform(|platform| platform.settings) as *const AstralSettingsHandle;
    match unsafe { store(handle) } {
        Some(mut guard) => work(&mut guard),
        None => fallback,
    }
}

fn has_settings() -> bool {
    platform(|platform| platform.settings != 0)
}

fn setting(key: &str) -> bool {
    with_settings(false, |settings| settings.get_boolean("astral", key, false))
}

fn store_setting(key: &str, value: Value) {
    with_settings((), |settings| {
        if settings.set("astral", key, value) {
            settings.save();
        }
    });
}

fn toggle(key: &str) {
    if has_settings() {
        store_setting(key, json!(!setting(key)));
    }
}

fn plugins() -> Vec<Value> {
    match host("plugins", &json!([])) {
        Value::Array(list) => list,
        _ => Vec::new(),
    }
}

fn objects(value: Value) -> Vec<Value> {
    match value {
        Value::Object(_) => vec![value],
        Value::Array(list) => list.into_iter().filter(Value::is_object).collect(),
        _ => Vec::new(),
    }
}

fn string(object: &Value, key: &str) -> String {
    object.get(key).and_then(Value::as_str).unwrap_or_default().to_owned()
}

fn string_or(object: &Value, key: &str, fallback: &str) -> String {
    object.get(key).and_then(Value::as_str).unwrap_or(fallback).to_owned()
}

fn flag(object: &Value, key: &str) -> bool {
    object.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn number(object: &Value, key: &str) -> f64 {
    object.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

fn array(object: &Value, key: &str) -> Vec<Value> {
    object.get(key).and_then(Value::as_array).cloned().unwrap_or_default()
}

fn row(key: &str, title: &str, icon: &str, kind: i64) -> Value {
    json!({"key": key, "title": title, "icon": icon, "kind": kind})
}

fn with(mut base: Value, extra: Value) -> Value {
    if let (Some(base), Value::Object(extra)) = (base.as_object_mut(), extra) {
        base.extend(extra);
    }
    base
}

fn section(title: &str, rows: Vec<Value>) -> Value {
    json!({"title": title, "rows": objects(Value::Array(rows))})
}

fn page(title: &str, sections: Vec<Value>) -> Value {
    let filled: Vec<Value> = sections.into_iter().filter(|section| !array(section, "rows").is_empty()).collect();
    json!({"title": title, "sections": objects(Value::Array(filled))})
}

fn plugin_with_id(identifier: &str) -> Option<Value> {
    plugins().into_iter().find(|plugin| string(plugin, "id") == identifier)
}

fn local_time(seconds: i64) -> String {
    let time = seconds as libc::time_t;
    let mut parts: libc::tm = unsafe { core::mem::zeroed() };
    let mut buffer = [0u8; 64];
    let written = unsafe {
        libc::localtime_r(&time, &mut parts);
        libc::strftime(buffer.as_mut_ptr().cast(), buffer.len(), c"%Y-%m-%d %H:%M".as_ptr(), &parts)
    };
    String::from_utf8_lossy(&buffer[..written]).into_owned()
}

fn report_time(report: &Value) -> String {
    let when = string(report, "when");
    if !when.is_empty() {
        return when;
    }
    local_time(number(report, "time") as i64)
}

fn reports_section() -> Value {
    let mut rows: Vec<Value> = host_list("crashReports")
        .iter()
        .take(5)
        .enumerate()
        .map(|(index, report)| {
            let icon = if string(report, "kind") == "plugin" {
                "res://ic_mana_puzzle_piece_retry"
            } else {
                "res://ic_mana_bug"
            };
            with(
                row(&format!("report.{index}"), &string(report, "title"), icon, ROW_LINK),
                json!({"subtitle": tr_with("ASTRAL_REPORT_TAP_TO_COPY", &[("time", report_time(report))])}),
            )
        })
        .collect();
    if rows.is_empty() {
        rows.push(row(
            "no-reports",
            tr("ASTRAL_NO_CRASHES_OR_PLUGIN_FAILURES"),
            "res://ic_mana_circle_check",
            ROW_INFO,
        ));
    }
    section(tr("ASTRAL_REPORTS"), rows)
}

struct Context {
    recovering: bool,
    discord_version: String,
    compatibility: String,
}

fn context() -> Context {
    let value = host("context", &json!([]));
    Context {
        recovering: flag(&value, "recovering"),
        discord_version: string(&value, "discordVersion"),
        compatibility: string(&value, "compatibility"),
    }
}

fn general_page(context: &Context) -> Value {
    let stats = astral_wire_stats();
    let developer = setting("developer-mode");
    let blocked = host("blockedRequests", &json!([])).as_u64().unwrap_or(0).to_string();
    let developer_subtitle = if developer {
        tr_with("ASTRAL_PAIRING_CODE", &[("code", host_text("pairingCode"))])
    } else {
        tr("ASTRAL_RUN_PLUGINS_THAT_ASTRAL_HAS_NOT").to_owned()
    };
    let staff_subtitle = if capable("restartGatewaySession") {
        tr("ASTRAL_UNLOCK_DISCORDS_STAFF_DEVTOOLS")
    } else {
        tr("ASTRAL_UNLOCK_DISCORDS_STAFF_DEVTOOLS_RELAUNCH_DISCORD")
    };
    page(
        "astral",
        vec![
            section(
                "astral",
                vec![
                    with(
                        row("version", tr("ASTRAL_VERSION"), "res://ic_mana_circle_information", ROW_INFO),
                        json!({"detail": version()}),
                    ),
                    with(
                        row("discord", "Discord", "res://ic_mana_mobile_phone_settings", ROW_INFO),
                        json!({"detail": context.discord_version}),
                    ),
                    with(
                        row("engine", tr("ASTRAL_ENGINE"), "res://ic_mana_settings", ROW_INFO),
                        json!({"detail": "Phoenix"}),
                    ),
                    with(
                        row("runtime", tr("ASTRAL_PLUGIN_RUNTIME"), "res://ic_mana_puzzle_piece", ROW_INFO),
                        json!({"detail": "Lua 5.4"}),
                    ),
                ],
            ),
            section(
                tr("ASTRAL_PREFERENCES"),
                vec![
                    with(
                        row("safe-mode", tr("ASTRAL_SAFE_MODE"), "res://ic_mana_shield", ROW_SWITCH),
                        json!({"on": setting("safe-mode"), "subtitle": tr("ASTRAL_START_WITHOUT_PLUGINS_ON_THE_NEXT")}),
                    ),
                    with(
                        row("developer-mode", tr("ASTRAL_DEVELOPER_MODE"), "res://ic_mana_bug", ROW_SWITCH),
                        json!({"on": developer, "subtitle": developer_subtitle}),
                    ),
                    with(
                        row("staff-devtools", tr("ASTRAL_STAFF_DEVTOOLS"), "res://ic_mana_wrench", ROW_SWITCH),
                        json!({"on": setting("staff-devtools"), "subtitle": staff_subtitle}),
                    ),
                    with(
                        row(
                            "force-phoenix",
                            tr("ASTRAL_FORCE_PHOENIX"),
                            "res://ic_mana_settings_arrow_up",
                            ROW_INFO,
                        ),
                        json!({"detail": tr("ASTRAL_ON"), "subtitle": tr("ASTRAL_THE_REACT_NATIVE_FALLBACK_STAYS_OFF")}),
                    ),
                ],
            ),
            section(
                tr("ASTRAL_BACKUP"),
                vec![
                    with(
                        row("export-backup", tr("ASTRAL_EXPORT_SETTINGS"), "res://ic_mana_download", ROW_LINK),
                        json!({"subtitle": tr("ASTRAL_SAVE_ASTRAL_AND_ADDON_SETTINGS_TO")}),
                    ),
                    with(
                        row("import-backup", tr("ASTRAL_IMPORT_SETTINGS"), "res://ic_mana_refresh", ROW_LINK),
                        json!({"subtitle": tr("ASTRAL_RESTORE_A_BACKUP_AND_REINSTALL_ITS")}),
                    ),
                ],
            ),
            reports_section(),
            section(
                tr("ASTRAL_DIAGNOSTICS"),
                vec![
                    with(
                        row("recovery", tr("ASTRAL_RECOVERY"), "res://ic_mana_refresh", ROW_INFO),
                        json!({"detail": if context.recovering { tr("ASTRAL_PLUGINS_PAUSED_AFTER_A_CRASH") } else { tr("ASTRAL_NORMAL") }}),
                    ),
                    with(
                        row("decorations", tr("ASTRAL_DECORATIONS"), "res://ic_mana_bug", ROW_INFO),
                        json!({
                            "detail": stats.tagged.to_string(),
                            "subtitle": format!("{} authors, {} users, {} channels resolved", stats.authors, stats.with_user, stats.with_channel),
                        }),
                    ),
                    with(
                        row("telemetry", tr("ASTRAL_TELEMETRY_BLOCKED"), "res://ic_mana_shield", ROW_INFO),
                        json!({"detail": blocked, "subtitle": tr("ASTRAL_REQUESTS_TO_DISCORDS_ANALYTICS_AND_SENTRY")}),
                    ),
                    with(
                        row("reload", tr("ASTRAL_RELOAD_PLUGINS"), "res://ic_mana_refresh", ROW_LINK),
                        json!({"subtitle": tr("ASTRAL_RESTART_EVERY_ENABLED_PLUGIN")}),
                    ),
                    with(
                        row("soft-restart", tr("ASTRAL_SOFT_RESTART"), "res://ic_mana_refresh", ROW_LINK),
                        json!({"subtitle": tr("ASTRAL_RESTART_ASTRALS_RUNTIME_THEME_AND_ICONS")}),
                    ),
                ],
            ),
        ],
    )
}

fn plugin_enabled(plugin: &Value) -> bool {
    flag(plugin, "enabled") && !flag(plugin, "failed")
}

fn filter() -> String {
    let current = FILTER.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
    if current.is_empty() { "all".into() } else { current }
}

fn matches_filter(plugin: &Value, current: &str) -> bool {
    match current {
        "enabled" => plugin_enabled(plugin),
        "disabled" => !plugin_enabled(plugin),
        _ => true,
    }
}

pub fn mana_asset(name: &str) -> String {
    if name.starts_with("res://") {
        return name.to_owned();
    }
    let Some(stem) = name.strip_suffix("Icon").filter(|stem| !stem.is_empty()) else {
        return "res://astral_plug".into();
    };
    let mut asset = String::from("res://ic_mana");
    let characters = stem.as_bytes();
    for (index, &character) in characters.iter().enumerate() {
        let previous_digit = index > 0 && characters[index - 1].is_ascii_digit();
        if character.is_ascii_uppercase() || (character.is_ascii_digit() && !previous_digit) {
            asset.push('_');
        }
        asset.push(character.to_ascii_lowercase() as char);
    }
    asset
}

fn plugin_icon(plugin: &Value) -> String {
    if !string(plugin, "iconSvg").is_empty()
        && let Some(registered) = host("pluginIconSVG", &json!([plugin])).as_str()
    {
        return registered.to_owned();
    }
    mana_asset(&string(plugin, "icon"))
}

fn permission_description(permission: &str) -> (String, &'static str) {
    match Capability::parse(permission) {
        Some(capability) => (capability.description().to_owned(), capability.icon()),
        None => (
            tr_with("ASTRAL_UNKNOWN_PERMISSION", &[("permission", permission.to_owned())]),
            "res://ic_mana_circle_information",
        ),
    }
}

fn author_names(plugin: &Value) -> String {
    array(plugin, "authors")
        .iter()
        .map(|author| string(author, "name"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn author_avatars(plugin: &Value) -> Vec<Value> {
    array(plugin, "authors")
        .iter()
        .take(3)
        .enumerate()
        .map(
            |(index, author)| match host("authorAvatar", &json!([string(author, "id"), index])) {
                Value::String(avatar) => Value::String(avatar),
                _ => Value::String(format!("https://cdn.discordapp.com/embed/avatars/{}.png", index % 6)),
            },
        )
        .collect()
}

fn plugin_card(plugin: &Value) -> Value {
    let version = string(plugin, "version");
    let mut card = json!({
        "key": format!("plugin.{}", string(plugin, "id")),
        "title": string(plugin, "name"),
        "authors": author_names(plugin),
        "avatars": author_avatars(plugin),
        "hasSettings": !array(plugin, "switches").is_empty(),
        "description": string(plugin, "description"),
        "version": if flag(plugin, "signed") { format!("v{version}") } else { tr_with("ASTRAL_VERSION_UNSIGNED", &[("version", version.clone())]) },
        "icon": plugin_icon(plugin),
        "on": plugin_enabled(plugin),
        "failed": flag(plugin, "failed"),
    });
    if flag(plugin, "failed") {
        card["error"] = json!(tr_with("ASTRAL_STOPPED_AFTER_ERRORS_DETAIL", &[("error", string(plugin, "error"))]));
    }
    card
}

fn plugins_page() -> Value {
    let plugins = plugins();
    let current = filter();
    let enabled = plugins.iter().filter(|plugin| plugin_enabled(plugin)).count();
    let cards: Vec<Value> = plugins
        .iter()
        .filter(|plugin| matches_filter(plugin, &current))
        .map(plugin_card)
        .collect();
    let chips = json!([
        {"key": "filter.all", "title": tr_with("ASTRAL_FILTER_ALL", &[("count", plugins.len().to_string())]), "selected": current == "all"},
        {"key": "filter.enabled", "title": tr_with("ASTRAL_FILTER_ENABLED", &[("count", enabled.to_string())]), "selected": current == "enabled"},
        {"key": "filter.disabled", "title": tr_with("ASTRAL_FILTER_DISABLED", &[("count", (plugins.len() - enabled).to_string())]), "selected": current == "disabled"},
    ]);
    let mut result = page(tr("ASTRAL_PLUGINS"), Vec::new());
    result["toolbar"] = json!({"chips": chips, "buttonKey": "browse", "buttonIcon": "res://ic_mana_download"});
    if cards.is_empty() {
        let empty = if plugins.is_empty() {
            tr("ASTRAL_NO_PLUGINS_INSTALLED")
        } else {
            tr("ASTRAL_NO_PLUGINS_MATCH_THIS_FILTER")
        };
        result["sections"] = json!([section(
            tr("ASTRAL_PLUGINS"),
            vec![row("none", empty, "res://ic_mana_filters_horizontal", ROW_INFO)]
        )]);
    }
    result["cards"] = Value::Array(cards);
    result
}

fn missing_plugin_page() -> Value {
    page(
        tr("ASTRAL_PLUGIN"),
        vec![section(
            tr("ASTRAL_PLUGIN"),
            vec![row(
                "missing",
                tr("ASTRAL_THIS_PLUGIN_IS_NO_LONGER_INSTALLED"),
                "res://ic_mana_puzzle_piece_minus",
                ROW_INFO,
            )],
        )],
    )
}

fn plugin_page(identifier: &str) -> Value {
    let Some(plugin) = plugin_with_id(identifier) else {
        return missing_plugin_page();
    };
    let mut about = Vec::new();
    if !string(&plugin, "description").is_empty() {
        about.push(row(
            "description",
            &string(&plugin, "description"),
            "res://ic_mana_circle_information",
            ROW_INFO,
        ));
    }
    about.push(with(
        row("plugin-version", tr("ASTRAL_VERSION"), "res://ic_mana_settings_info", ROW_INFO),
        json!({"detail": string(&plugin, "version")}),
    ));
    if !array(&plugin, "authors").is_empty() {
        about.push(with(
            row("authors", tr("ASTRAL_AUTHORS"), "res://ic_mana_user", ROW_INFO),
            json!({"subtitle": author_names(&plugin)}),
        ));
    }
    about.push(with(
        row("source", tr("ASTRAL_SOURCE"), "res://ic_mana_download", ROW_INFO),
        json!({"detail": if flag(&plugin, "bundled") { tr("ASTRAL_BUNDLED") } else { tr("ASTRAL_MARKETPLACE") }}),
    ));
    let mut permissions: Vec<Value> = array(&plugin, "permissions")
        .iter()
        .filter_map(Value::as_str)
        .map(|permission| {
            let (description, icon) = permission_description(permission);
            row(&format!("permission.{permission}"), &description, icon, ROW_INFO)
        })
        .collect();
    if permissions.is_empty() {
        permissions.push(row(
            "permission.none",
            tr("ASTRAL_NO_ACCESS_TO_DISCORD_DATA"),
            "res://ic_mana_shield",
            ROW_INFO,
        ));
    }
    let mut status = vec![with(
        row("enabled", tr("ASTRAL_ENABLED"), "res://ic_mana_puzzle_piece", ROW_SWITCH),
        json!({"on": plugin_enabled(&plugin)}),
    )];
    if flag(&plugin, "failed") {
        status.push(with(
            row("error", tr("ASTRAL_STOPPED_AFTER_ERRORS"), "res://ic_mana_bug", ROW_INFO),
            json!({"subtitle": string(&plugin, "error")}),
        ));
        status.push(row("retry", tr("ASTRAL_TRY_AGAIN"), "res://ic_mana_refresh", ROW_LINK));
    }
    let mut manage = vec![reset_row()];
    if !flag(&plugin, "bundled") {
        manage.push(row("uninstall", tr("ASTRAL_UNINSTALL"), "res://ic_mana_trash", ROW_LINK));
    }
    page(
        &string(&plugin, "name"),
        vec![
            section(tr("ASTRAL_ABOUT"), about),
            section(tr("ASTRAL_PERMISSIONS"), permissions),
            section(tr("ASTRAL_STATUS"), status),
            section(tr("ASTRAL_MANAGE"), manage),
        ],
    )
}

fn reset_row() -> Value {
    with(
        row("reset", tr("ASTRAL_RESET_SETTINGS"), "res://ic_mana_refresh", ROW_LINK),
        json!({"subtitle": tr("ASTRAL_RETURN_EVERY_OPTION_TO_ITS_DEFAULT")}),
    )
}

fn setting_icon(entry: &Value) -> String {
    let icon = string(entry, "icon");
    if icon.is_empty() || icon.starts_with("res://") || icon.starts_with("https://") {
        return icon;
    }
    mana_asset(&icon)
}

const COLOR_PRESETS: [(&str, u32); 14] = [
    ("Blurple", 0xFF58_65F2),
    ("ASTRAL_BLUE", 0xFF3B_82F6),
    ("ASTRAL_CYAN", 0xFF06_B6D4),
    ("ASTRAL_TEAL", 0xFF14_B8A6),
    ("ASTRAL_GREEN", 0xFF23_A55A),
    ("ASTRAL_LIME", 0xFF84_CC16),
    ("ASTRAL_YELLOW", 0xFFF0_B232),
    ("ASTRAL_ORANGE", 0xFFF9_7316),
    ("ASTRAL_RED", 0xFFF2_3F43),
    ("ASTRAL_PINK", 0xFFEC_4899),
    ("ASTRAL_PURPLE", 0xFF8B_5CF6),
    ("ASTRAL_GREY", 0xFF80_848E),
    ("ASTRAL_WHITE", 0xFFFF_FFFF),
    ("ASTRAL_BLACK", 0xFF00_0000),
];

fn swatch(argb: u32) -> String {
    String::from_utf8_lossy(&swatch_uri(argb)).into_owned()
}

fn color_icon(value: &str) -> String {
    parse_color(value.as_bytes()).map_or_else(|| "res://ic_mana_paint_palette".to_owned(), swatch)
}

fn is_color_text(value: &str) -> bool {
    value.is_empty() || parse_color(value.as_bytes()).is_some()
}

fn color_row(entry: &Value) -> Value {
    let value = string(entry, "value");
    let detail = if value.is_empty() {
        tr("ASTRAL_AUTOMATIC").to_owned()
    } else {
        value.clone()
    };
    with(
        row(
            &format!("color.{}", string(entry, "key")),
            &string(entry, "title"),
            &color_icon(&value),
            ROW_LINK,
        ),
        json!({"detail": detail}),
    )
}

fn text_setting_for(plugin: &Value, key: &str) -> Option<Value> {
    array(plugin, "textSettings").into_iter().find(|entry| string(entry, "key") == key)
}

fn split_target(target: &str) -> Option<(&str, &str)> {
    target.rsplit_once(':')
}

fn plugin_color_page(target: &str) -> Value {
    let Some((identifier, key)) = split_target(target) else {
        return plugin_page(target);
    };
    let Some(entry) = plugin_with_id(identifier).and_then(|plugin| text_setting_for(&plugin, key)) else {
        return plugin_page(identifier);
    };
    let value = string(&entry, "value");
    let current = parse_color(value.as_bytes());
    let mut matched = value.is_empty();
    let mut presets = vec![row(
        "swatch.auto",
        tr("ASTRAL_AUTOMATIC"),
        "res://ic_mana_paint_palette",
        if value.is_empty() { ROW_SELECTED } else { ROW_CHOICE },
    )];
    for (name, argb) in COLOR_PRESETS {
        let hex = color_text(argb);
        let selected = current == Some(argb);
        matched = matched || selected;
        presets.push(row(
            &format!("swatch.{}", &hex[1..]),
            tr(name),
            &swatch(argb),
            if selected { ROW_SELECTED } else { ROW_CHOICE },
        ));
    }
    let custom = vec![with(
        row("hex", tr("ASTRAL_HEX_CODE"), &color_icon(&value), ROW_LINK),
        json!({"detail": if matched { tr("ASTRAL_EDIT").to_owned() } else { value.clone() }, "subtitle": tr("ASTRAL_HEX_HINT")}),
    )];
    page(
        &string(&entry, "title"),
        vec![section(tr("ASTRAL_COLORS"), presets), section(tr("ASTRAL_CUSTOM"), custom)],
    )
}

fn plugin_color_action(target: &str, key: &str, changed: &Changed) -> Option<String> {
    let (identifier, setting_key) = split_target(target)?;
    let Some(entry) = plugin_with_id(identifier).and_then(|plugin| text_setting_for(&plugin, setting_key)) else {
        return Some("..".into());
    };
    if key == "swatch.auto" {
        set_text(identifier, setting_key, "");
    } else if let Some(hex) = key.strip_prefix("swatch.") {
        set_text(identifier, setting_key, &format!("#{hex}"));
    } else if key == "hex" && capable("editText") {
        let (identifier, setting_key, changed) = (identifier.to_owned(), setting_key.to_owned(), changed.clone());
        host_reply(
            "editText",
            &json!([string(&entry, "title"), string(&entry, "value"), 9]),
            Arc::new(move |edited: Option<String>| {
                if let Some(edited) = edited.filter(|edited| is_color_text(edited)) {
                    set_text(&identifier, &setting_key, &edited);
                }
                changed();
            }),
        );
    }
    None
}

fn set_text(identifier: &str, key: &str, value: &str) {
    host("setText", &json!([identifier, key, value]));
}

fn format_number(value: f64) -> String {
    if value == value.floor() {
        return format!("{value:.0}");
    }
    general(value)
}

fn trim_fraction(text: &str) -> &str {
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.')
    } else {
        text
    }
}

pub fn general(value: f64) -> String {
    if !value.is_finite() {
        return value.to_string();
    }
    if value == 0.0 {
        return "0".to_owned();
    }
    let exponent = value.abs().log10().floor() as i32;
    if !(GENERAL_MIN_EXPONENT..GENERAL_DIGITS).contains(&exponent) {
        let scientific = format!("{:.*e}", (GENERAL_DIGITS - 1) as usize, value);
        let (mantissa, power) = scientific.split_once('e').unwrap_or((&scientific, "0"));
        let power: i32 = power.parse().unwrap_or(0);
        let sign = if power < 0 { '-' } else { '+' };
        return format!("{}e{sign}{:02}", trim_fraction(mantissa), power.abs());
    }
    let decimals = (GENERAL_DIGITS - 1 - exponent).max(0) as usize;
    trim_fraction(&format!("{value:.decimals$}")).to_owned()
}

const GENERAL_DIGITS: i32 = 6;
const GENERAL_MIN_EXPONENT: i32 = -4;

fn number_row(entry: &Value) -> Value {
    let (minimum, step, value) = (number(entry, "min"), number(entry, "step"), number(entry, "value"));
    let steps = ((number(entry, "max") - minimum) / step).round() as i32;
    let position = ((value - minimum) / step).round() as i32;
    with(
        row(
            &format!("number.{}", string(entry, "key")),
            &string(entry, "title"),
            &setting_icon(entry),
            ROW_NUMBER,
        ),
        json!({"detail": format_number(value), "steps": steps, "position": position}),
    )
}

fn setting_row(entry: &Value, text_setting: bool) -> Value {
    let key = string(entry, "key");
    let title = string(entry, "title");
    let mut result = if flag(entry, "button") {
        with(
            row(&format!("button.{key}"), &title, &setting_icon(entry), ROW_LINK),
            json!({"detail": string(entry, "detail")}),
        )
    } else if text_setting && flag(entry, "color") {
        color_row(entry)
    } else if text_setting {
        with(
            row(&format!("text.{key}"), &title, &setting_icon(entry), ROW_LINK),
            json!({"detail": tr("ASTRAL_EDIT")}),
        )
    } else if flag(entry, "number") {
        number_row(entry)
    } else {
        with(
            row(&format!("setting.{key}"), &title, &setting_icon(entry), ROW_SWITCH),
            json!({"on": flag(entry, "on")}),
        )
    };
    let subtitle = string(entry, "subtitle");
    if !subtitle.is_empty() {
        result["subtitle"] = json!(subtitle);
    }
    result
}

fn choice_setting_rows(entry: &Value) -> Vec<Value> {
    array(entry, "options")
        .iter()
        .map(|option| {
            let selected = string(option, "value") == string(entry, "value");
            let mut choice = row(
                &format!("choice.{}.{}", string(entry, "key"), string(option, "value")),
                &string(option, "title"),
                "",
                if selected { ROW_SELECTED } else { ROW_CHOICE },
            );
            let subtitle = string(option, "subtitle");
            if !subtitle.is_empty() {
                choice["subtitle"] = json!(subtitle);
            }
            choice
        })
        .collect()
}

fn plugin_settings_page(identifier: &str) -> Value {
    let Some(plugin) = plugin_with_id(identifier) else {
        return plugin_page(identifier);
    };
    let mut entries: Vec<(Value, bool)> = Vec::new();
    entries.extend(array(&plugin, "switches").into_iter().map(|entry| (entry, false)));
    entries.extend(array(&plugin, "textSettings").into_iter().map(|entry| (entry, true)));
    entries.extend(array(&plugin, "settingsButtons").into_iter().map(|entry| (entry, false)));
    entries.sort_by(|left, right| {
        number(&left.0, "order")
            .partial_cmp(&number(&right.0, "order"))
            .unwrap_or(core::cmp::Ordering::Equal)
    });
    let mut order: Vec<String> = Vec::new();
    let mut grouped: HashMap<String, Vec<Value>> = HashMap::new();
    let mut titles: HashMap<String, String> = HashMap::new();
    for (entry, text_setting) in &entries {
        if !text_setting && !array(entry, "options").is_empty() {
            let choice = format!("choice:{}", string(entry, "key"));
            grouped.insert(choice.clone(), choice_setting_rows(entry));
            titles.insert(choice.clone(), string(entry, "title"));
            order.push(choice);
            continue;
        }
        let name = Some(string_or(entry, "section", tr("ASTRAL_SETTINGS")))
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| tr("ASTRAL_SETTINGS").into());
        if !grouped.contains_key(&name) {
            grouped.insert(name.clone(), Vec::new());
            order.push(name.clone());
        }
        grouped.entry(name).or_default().push(setting_row(entry, *text_setting));
    }
    let mut sections: Vec<Value> = order
        .iter()
        .map(|name| section(titles.get(name).unwrap_or(name), grouped.get(name).cloned().unwrap_or_default()))
        .collect();
    sections.push(section(tr("ASTRAL_MANAGE"), vec![reset_row()]));
    page(&string(&plugin, "name"), sections)
}

fn choice_rows(items: &[Value], active: &str, prefix: &str, icon: &str) -> Vec<Value> {
    items
        .iter()
        .map(|item| {
            let id = string(item, "id");
            let mut choice = row(
                &format!("{prefix}{id}"),
                &string(item, "name"),
                icon,
                if id == active { ROW_SELECTED } else { ROW_CHOICE },
            );
            if let Some(description) = item.get("description").filter(|description| description.is_string()) {
                choice["subtitle"] = description.clone();
            }
            choice
        })
        .collect()
}

const APP_ICON_PREFIX: &str = "app-icon.";

fn app_icon_title(name: &str) -> String {
    let variant = name.strip_prefix("Astral").unwrap_or(name).strip_suffix("Icon").unwrap_or(name);
    if variant.is_empty() {
        "astral".into()
    } else {
        format!("astral {variant}")
    }
}

fn app_icon_rows() -> Vec<Value> {
    let state = host("appIcons", &json!([]));
    let icons = array(&state, "icons");
    if !flag(&state, "supported") || icons.is_empty() {
        return Vec::new();
    }
    let current = string(&state, "current");
    let choice = |key: &str, title: &str, selected: bool| {
        row(
            &format!("{APP_ICON_PREFIX}{key}"),
            title,
            "res://astral_galaxy",
            if selected { ROW_SELECTED } else { ROW_CHOICE },
        )
    };
    let mut rows = vec![choice("", "Discord", current.is_empty())];
    rows.extend(
        icons
            .iter()
            .filter_map(Value::as_str)
            .map(|name| choice(name, &app_icon_title(name), current == name)),
    );
    rows
}

fn design_page() -> Value {
    let themes = choice_rows(
        &host_list("themes"),
        &host_text("activeTheme"),
        "theme.",
        "res://ic_mana_paint_palette",
    );
    let active_pack = host_text("activeIconPack");
    let packs: Vec<Value> = host_list("iconPacks")
        .iter()
        .map(|pack| {
            let id = string(pack, "id");
            let mut choice = row(
                &format!("icon-pack.{id}"),
                &string(pack, "name"),
                "res://ic_mana_image",
                if id == active_pack { ROW_SELECTED } else { ROW_CHOICE },
            );
            let description = string(pack, "description");
            let count = number(pack, "count") as i64;
            choice["subtitle"] = json!(if count != 0 {
                format!("{description} · {count} icons")
            } else {
                description
            });
            choice
        })
        .collect();
    let mut emoji = choice_rows(
        &host_list("emojiPacks"),
        &host_text("activeEmojiPack"),
        "emoji-pack.",
        "res://ic_mana_reaction",
    );
    emoji.push(with(
        row("emoji-animated", tr("ASTRAL_ANIMATED_EMOJI"), "res://ic_mana_sparkles", ROW_SWITCH),
        json!({"on": host("emojiAnimated", &json!([])).as_bool().unwrap_or(false), "subtitle": tr("ASTRAL_USE_A_PACKS_ANIMATED_EMOJI_WHERE")}),
    ));
    page(
        tr("ASTRAL_DESIGN"),
        vec![
            section(tr("ASTRAL_THEMES"), themes),
            section(tr("ASTRAL_APP_ICON"), app_icon_rows()),
            section(tr("ASTRAL_ICON_PACKS"), packs),
            section(tr("ASTRAL_EMOJI_PACKS"), emoji),
            section(
                tr("ASTRAL_HOW_THEMES_APPLY"),
                vec![with(
                    row(
                        "theme-note",
                        tr("ASTRAL_RECOLOURS_PHOENIX_VIEWS_RIGHT_AWAY"),
                        "res://ic_mana_circle_information",
                        ROW_INFO,
                    ),
                    json!({"subtitle": tr("ASTRAL_COLOURS_CHANGE_ON_SCREEN_AS_SOON")}),
                )],
            ),
            section(
                tr("ASTRAL_GET_MORE"),
                vec![
                    with(
                        row(
                            "import-unbound-theme",
                            tr("ASTRAL_IMPORT_UNBOUND_THEME"),
                            "res://ic_mana_download",
                            ROW_LINK,
                        ),
                        json!({"subtitle": tr("ASTRAL_CONVERT_AN_UNBOUND_THEME_FILE_INTO")}),
                    ),
                    row("browse", tr("ASTRAL_BROWSE_THEMES"), "res://ic_mana_shop", ROW_LINK),
                ],
            ),
        ],
    )
}

fn installed_version(identifier: &str, kind: &str) -> Option<String> {
    if kind == "theme" {
        return host_list("themes")
            .iter()
            .find(|theme| string(theme, "id") == identifier)
            .map(|theme| string(theme, "version"));
    }
    plugin_with_id(identifier).map(|plugin| string(&plugin, "version"))
}

fn install_status(identifier: &str) -> Option<String> {
    host("installStatus", &json!([identifier])).as_str().map(str::to_owned)
}

fn package_icon(package: &Value, identifier: &str) -> String {
    if let Some(plugin) = plugin_with_id(identifier) {
        return plugin_icon(&plugin);
    }
    let icon = string(package, "icon");
    if !icon.is_empty() {
        mana_asset(&icon)
    } else {
        "res://ic_mana_puzzle_piece_plus".into()
    }
}

fn package_row(package: &Value, kind: &str) -> Value {
    let identifier = string(package, "id");
    let installed = installed_version(&identifier, kind);
    let status = install_status(&identifier).unwrap_or_else(|| match &installed {
        None => tr("ASTRAL_GET").into(),
        Some(installed) if version_is_newer(&string(package, "version"), installed) => tr("ASTRAL_UPDATE").into(),
        Some(_) => tr("ASTRAL_INSTALLED").into(),
    });
    let icon = if kind == "theme" {
        "res://ic_mana_paint_palette".to_owned()
    } else {
        package_icon(package, &identifier)
    };
    let mut subtitle = string(package, "description");
    if package.get("version").is_some_and(Value::is_string) {
        subtitle = format!("v{} · {subtitle}", string(package, "version"));
    }
    with(
        row(&format!("{kind}.{identifier}"), &string(package, "name"), &icon, ROW_LINK),
        json!({"detail": status, "subtitle": subtitle}),
    )
}

fn catalog_status_rows() -> Vec<Value> {
    match host_text("catalogState").as_str() {
        "loading" => vec![row("loading", tr("ASTRAL_LOADING_THE_CATALOG"), "res://ic_mana_refresh", ROW_INFO)],
        "failed" => vec![with(
            row("failed", tr("ASTRAL_COULD_NOT_LOAD_THE_CATALOG"), "res://ic_mana_bug", ROW_INFO),
            json!({"subtitle": host_text("catalogError")}),
        )],
        _ => Vec::new(),
    }
}

fn catalog() -> Value {
    let value = host("catalog", &json!([]));
    if value.is_object() { value } else { json!({}) }
}

fn marketplace_page() -> Value {
    let packages = catalog();
    let mut sections = Vec::new();
    let status = catalog_status_rows();
    if !status.is_empty() {
        sections.push(section(tr("ASTRAL_CATALOG"), status));
    }
    for kind in ["plugin", "theme"] {
        let rows: Vec<Value> = array(&packages, &format!("{kind}s"))
            .iter()
            .map(|package| package_row(package, kind))
            .collect();
        if !rows.is_empty() {
            sections.push(section(
                if kind == "plugin" {
                    tr("ASTRAL_PLUGINS")
                } else {
                    tr("ASTRAL_THEMES")
                },
                rows,
            ));
        }
    }
    sections.push(section(
        tr("ASTRAL_SOURCE"),
        vec![
            with(
                row("catalog-url", tr("ASTRAL_CATALOG"), "res://ic_mana_link", ROW_INFO),
                json!({"subtitle": host_text("catalogURL")}),
            ),
            row("refresh", tr("ASTRAL_REFRESH"), "res://ic_mana_refresh", ROW_LINK),
        ],
    ));
    page(tr("ASTRAL_MARKETPLACE"), sections)
}

fn pending_updates() -> Vec<(Value, &'static str)> {
    let packages = catalog();
    let mut pending = Vec::new();
    for kind in ["plugin", "theme"] {
        for package in array(&packages, &format!("{kind}s")) {
            if installed_version(&string(&package, "id"), kind)
                .is_some_and(|installed| version_is_newer(&string(&package, "version"), &installed))
            {
                pending.push((package, kind));
            }
        }
    }
    pending
}

fn auto_updates() -> bool {
    with_settings(true, |settings| settings.get_boolean("astral", "auto-update", true))
}

fn latest_astral_version() -> String {
    catalog()
        .get("astral")
        .filter(|astral| astral.is_object())
        .map(|astral| string(astral, "version"))
        .unwrap_or_default()
}

fn notice_astral_update() {
    let latest = latest_astral_version();
    if latest.is_empty() || !version_is_newer(&latest, &version()) || !has_settings() {
        return;
    }
    if with_settings(None, |settings| settings.get_string("astral", "update-notice")).as_deref() == Some(latest.as_str()) {
        return;
    }
    store_setting("update-notice", json!(latest));
    host(
        "notify",
        &json!([
            tr_with("ASTRAL_UPDATE_AVAILABLE", &[("version", latest.to_string())]),
            tr("ASTRAL_SEE_SETTINGS_ASTRAL_UPDATES")
        ]),
    );
}

fn install_pending_updates(changed: &Changed) {
    if !capable("installPackage") {
        return;
    }
    for (package, kind) in pending_updates() {
        host_with("installPackage", &json!([package, kind]), changed.clone());
    }
}

fn catalog_refreshed(changed: &Changed) {
    if auto_updates() {
        install_pending_updates(changed);
    }
    notice_astral_update();
    changed();
}

fn last_catalog_refresh() -> i64 {
    with_settings(None, |settings| settings.get("astral", "catalog-refreshed-at"))
        .and_then(|value| value.as_i64())
        .unwrap_or(0)
}

fn updates_page(context: &Context) -> Value {
    let latest = latest_astral_version();
    let installed = version();
    let astral_status = if latest.is_empty() {
        tr("ASTRAL_UNKNOWN").to_owned()
    } else if version_is_newer(&latest, &installed) {
        tr_with("ASTRAL_UPDATE_TO", &[("version", latest.to_string())])
    } else {
        tr("ASTRAL_UP_TO_DATE").to_owned()
    };
    let mut addons: Vec<Value> = pending_updates()
        .iter()
        .map(|(package, kind)| {
            let identifier = string(package, "id");
            let status = install_status(&identifier).unwrap_or_else(|| format!("v{}", string(package, "version")));
            with(
                row(
                    &format!("{kind}.{identifier}"),
                    &string(package, "name"),
                    "res://ic_mana_cloud_download",
                    ROW_LINK,
                ),
                json!({"detail": status, "subtitle": tr_with("ASTRAL_INSTALLED_VERSION", &[("version", installed_version(&identifier, kind).unwrap_or_default())])}),
            )
        })
        .collect();
    if addons.len() > 1 {
        addons.push(row("update-all", tr("ASTRAL_UPDATE_ALL"), "res://ic_mana_download", ROW_LINK));
    }
    if addons.is_empty() {
        addons.push(row(
            "current",
            tr("ASTRAL_EVERYTHING_IS_UP_TO_DATE"),
            "res://ic_mana_circle_check",
            ROW_INFO,
        ));
    }
    let mut sections = Vec::new();
    let status = catalog_status_rows();
    if !status.is_empty() {
        sections.push(section(tr("ASTRAL_CATALOG"), status));
    }
    sections.push(section(
        "astral",
        vec![with(
            row("astral", "astral", "res://ic_mana_settings", ROW_INFO),
            json!({"detail": astral_status, "subtitle": tr_with("ASTRAL_INSTALLED_DETAIL", &[("version", installed.to_string())])}),
        )],
    ));
    sections.push(section(
        "Discord",
        vec![with(
            row("discord-build", tr("ASTRAL_BUILD"), "res://ic_mana_mobile_phone_settings", ROW_INFO),
            json!({"detail": if context.compatibility == "ready" { tr("ASTRAL_SUPPORTED") } else { tr("ASTRAL_UNSUPPORTED") }, "subtitle": context.discord_version}),
        )],
    ));
    sections.push(section(tr("ASTRAL_ADDONS"), addons));
    sections.push(section(
        tr("ASTRAL_CHECK"),
        vec![
            row("refresh", tr("ASTRAL_CHECK_FOR_UPDATES"), "res://ic_mana_refresh", ROW_LINK),
            with(
                row(
                    "auto-update",
                    tr("ASTRAL_UPDATE_AUTOMATICALLY"),
                    "res://ic_mana_download",
                    ROW_SWITCH,
                ),
                json!({"on": auto_updates(), "subtitle": tr("ASTRAL_INSTALL_ADDON_UPDATES_WHEN_DISCORD_OPENS")}),
            ),
        ],
    ));
    page(tr("ASTRAL_UPDATES"), sections)
}

fn catalog_package(kind: &str, identifier: &str) -> Option<Value> {
    array(&catalog(), &format!("{kind}s"))
        .into_iter()
        .find(|package| string(package, "id") == identifier)
}

fn install_from_key(key: &str, changed: &Changed) {
    let Some((kind, identifier)) = key.split_once('.') else { return };
    if !capable("installPackage") {
        return;
    }
    let Some(package) = catalog_package(kind, identifier) else { return };
    let installed = installed_version(identifier, kind);
    if installed.is_none_or(|installed| version_is_newer(&string(&package, "version"), &installed)) {
        host_with("installPackage", &json!([package, kind]), changed.clone());
    }
}

pub fn link_package(kind: &str, identifier: &str) -> Option<(String, String)> {
    let package = catalog_package(kind, identifier)?;
    Some((string(&package, "name"), string(&package, "description")))
}

pub fn install_linked(kind: &str, identifier: &str) {
    install_from_key(
        &format!("{kind}.{identifier}"),
        &(Arc::new(crate::settings::navigator::refresh) as Changed),
    );
}

fn set_enabled(identifier: &str, enabled: bool) {
    host("setEnabled", &json!([identifier, enabled]));
}

fn toggle_enabled(identifier: &str, plugin: &Value) {
    set_enabled(identifier, flag(plugin, "failed") || !flag(plugin, "enabled"));
}

fn plugin_action(identifier: &str, key: &str, changed: &Changed) -> Option<String> {
    let Some(plugin) = plugin_with_id(identifier) else {
        return Some("..".into());
    };
    if key == "switch.enabled" || key == "retry" {
        toggle_enabled(identifier, &plugin);
    } else if let Some(setting_key) = key.strip_prefix("switch.setting.") {
        for entry in array(&plugin, "switches")
            .iter()
            .filter(|entry| string(entry, "key") == setting_key)
        {
            host("setSwitch", &json!([identifier, setting_key, !flag(entry, "on")]));
        }
    } else if let Some(button) = key.strip_prefix("button.") {
        host("pressSettingsButton", &json!([identifier, button]));
        changed();
    } else if let Some(rest) = key.strip_prefix("number.") {
        if let Some((setting_key, position)) = rest.rsplit_once('=') {
            let position = leading_integer(position);
            for entry in array(&plugin, "switches")
                .iter()
                .filter(|entry| flag(entry, "number") && string(entry, "key") == setting_key)
            {
                host(
                    "setNumber",
                    &json!([
                        identifier,
                        setting_key,
                        number(entry, "min") + position as f64 * number(entry, "step")
                    ]),
                );
            }
        }
    } else if let Some(setting_key) = key.strip_prefix("text.") {
        if capable("editText")
            && let Some(entry) = array(&plugin, "textSettings")
                .into_iter()
                .find(|entry| string(entry, "key") == setting_key)
        {
            let maximum = number(&entry, "maxLength").max(0.0) as usize;
            let (identifier, setting_key, changed) = (identifier.to_owned(), setting_key.to_owned(), changed.clone());
            host_reply(
                "editText",
                &json!([string(&entry, "title"), string(&entry, "value"), maximum]),
                Arc::new(move |edited: Option<String>| {
                    if let Some(edited) = edited {
                        set_text(&identifier, &setting_key, &edited);
                    }
                    changed();
                }),
            );
        }
    } else if let Some(setting_key) = key.strip_prefix("color.") {
        return Some(format!("{PLUGIN_COLOR_ROUTE}{identifier}:{setting_key}"));
    } else if let Some(rest) = key.strip_prefix("choice.") {
        if let Some((setting_key, value)) = rest.split_once('.') {
            host("setChoice", &json!([identifier, setting_key, value]));
        }
    } else if key == "reset" {
        host("resetSettings", &json!([identifier]));
    } else if key == "uninstall" && !flag(&plugin, "bundled") {
        let _ = std::fs::remove_dir_all(string(&plugin, "directory"));
        host("removeStorage", &json!([identifier]));
        host("remove", &json!([identifier]));
        host("rescan", &json!([]));
        changed();
        return Some("..".into());
    }
    None
}

fn leading_integer(text: &str) -> i64 {
    let trimmed = text.trim_start();
    let sign = if trimmed.starts_with('-') { -1 } else { 1 };
    let digits: String = trimmed
        .trim_start_matches(['-', '+'])
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse::<i64>().map_or(0, |value| value * sign)
}

pub fn catalog_refresh_due(now: i64, last: i64) -> bool {
    last <= 0 || now < last || now - last >= CATALOG_REFRESH_SECONDS
}

pub fn refresh_catalog_if_due(now: i64, force: bool, changed: Changed) {
    if !capable("refreshCatalog") || (!force && !catalog_refresh_due(now, last_catalog_refresh())) {
        return;
    }
    store_setting("catalog-refreshed-at", json!(now));
    host_with("refreshCatalog", &json!([]), Arc::new(move || catalog_refreshed(&changed)));
}

pub fn category() -> Value {
    json!({"title": "astral", "rows": [
        {"key": "general", "title": "astral", "icon": "res://astral_galaxy"},
        {"key": "plugins", "title": tr("ASTRAL_PLUGINS"), "icon": "res://ic_mana_puzzle_piece"},
        {"key": "design", "title": tr("ASTRAL_DESIGN"), "icon": "res://ic_mana_paintbrush_thick"},
        {"key": "marketplace", "title": tr("ASTRAL_MARKETPLACE"), "icon": "res://ic_mana_shop"},
        {"key": "updates", "title": tr("ASTRAL_UPDATES"), "icon": "res://ic_mana_download"},
    ]})
}

pub fn version_is_newer(candidate: &str, installed: &str) -> bool {
    if candidate.is_empty() {
        return false;
    }
    if installed.is_empty() {
        return true;
    }
    let (left, right) = (candidate.as_bytes(), installed.as_bytes());
    let (mut left_index, mut right_index) = (0, 0);
    while left_index < left.len() || right_index < right.len() {
        if left_index < left.len() && right_index < right.len() && left[left_index].is_ascii_digit() && right[right_index].is_ascii_digit()
        {
            let left_end = left_index + left[left_index..].iter().take_while(|byte| byte.is_ascii_digit()).count();
            let right_end = right_index + right[right_index..].iter().take_while(|byte| byte.is_ascii_digit()).count();
            let left_value = digits_value(&left[left_index..left_end]);
            let right_value = digits_value(&right[right_index..right_end]);
            if left_value != right_value {
                return left_value > right_value;
            }
            (left_index, right_index) = (left_end, right_end);
            continue;
        }
        if left_index >= left.len() {
            return false;
        }
        if right_index >= right.len() {
            return true;
        }
        if left[left_index] != right[right_index] {
            return left[left_index] > right[right_index];
        }
        left_index += 1;
        right_index += 1;
    }
    false
}

fn digits_value(digits: &[u8]) -> u64 {
    std::str::from_utf8(digits)
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or(u64::MAX)
}

pub fn model(route: &str) -> Value {
    let context = context();
    if route == "general" {
        return general_page(&context);
    }
    if route == "plugins" || route.is_empty() {
        return plugins_page();
    }
    if let Some(target) = route.strip_prefix(PLUGIN_COLOR_ROUTE) {
        return plugin_color_page(target);
    }
    if let Some(identifier) = route.strip_prefix(PLUGIN_SETTINGS_ROUTE) {
        return plugin_settings_page(identifier);
    }
    if let Some(identifier) = route.strip_prefix(PLUGIN_ROUTE) {
        return plugin_page(identifier);
    }
    match route {
        "design" => design_page(),
        "marketplace" => marketplace_page(),
        "updates" => updates_page(&context),
        _ => page("astral", Vec::new()),
    }
}

fn simple(method: &str, arguments: &Value) -> Option<String> {
    host(method, arguments);
    None
}

pub fn action(route: &str, key: &str, changed: Changed) -> Option<String> {
    if let Some(target) = route.strip_prefix(PLUGIN_COLOR_ROUTE) {
        return plugin_color_action(target, key, &changed);
    }
    if let Some(identifier) = route.strip_prefix(PLUGIN_SETTINGS_ROUTE) {
        return plugin_action(identifier, key, &changed);
    }
    if let Some(identifier) = route.strip_prefix(PLUGIN_ROUTE) {
        return plugin_action(identifier, key, &changed);
    }
    if key == "browse" {
        return Some("marketplace".into());
    }
    if let Some(identifier) = key.strip_prefix("switch.plugin.") {
        if let Some(plugin) = plugin_with_id(identifier) {
            toggle_enabled(identifier, &plugin);
        }
        return None;
    }
    if let Some(identifier) = key.strip_prefix("settings.plugin.") {
        return Some(format!("{PLUGIN_SETTINGS_ROUTE}{identifier}"));
    }
    if let Some(identifier) = key.strip_prefix("more.plugin.") {
        return Some(format!("{PLUGIN_ROUTE}{identifier}"));
    }
    if let Some(current) = key.strip_prefix("filter.") {
        *FILTER.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = current.to_owned();
        return None;
    }
    match key {
        "import-unbound-theme" => {
            host_with("importUnboundTheme", &json!([]), changed);
            return None;
        }
        "export-backup" => return simple("exportBackup", &json!([])),
        "import-backup" => {
            host_with("importBackup", &json!([]), changed);
            return None;
        }
        "switch.developer-mode" => {
            toggle("developer-mode");
            for plugin in plugins().iter().filter(|plugin| flag(plugin, "enabled") && !flag(plugin, "signed")) {
                set_enabled(&string(plugin, "id"), true);
            }
            return None;
        }
        "switch.auto-update" => {
            if has_settings() {
                store_setting("auto-update", json!(!auto_updates()));
            }
            return None;
        }
        "switch.staff-devtools" => {
            toggle("staff-devtools");
            return simple("restartGatewaySession", &json!([]));
        }
        "switch.safe-mode" => {
            toggle("safe-mode");
            return None;
        }
        "soft-restart" => return simple("softRestart", &json!([])),
        "reload" => {
            for plugin in plugins().iter().filter(|plugin| flag(plugin, "enabled")) {
                set_enabled(&string(plugin, "id"), true);
            }
            return None;
        }
        "refresh" => {
            refresh_catalog_if_due(crate::support::bridge::epoch_ms() / 1000, true, changed);
            return None;
        }
        "update-all" => {
            install_pending_updates(&changed);
            return None;
        }
        _ => {}
    }
    if let Some(index) = key.strip_prefix("report.") {
        return simple("openReport", &json!([leading_integer(index).max(0)]));
    }
    if route == "design" {
        if let Some(pack) = key.strip_prefix("emoji-pack.") {
            return simple("selectEmojiPack", &json!([pack]));
        }
        if key == "switch.emoji-animated" {
            let animated = host("emojiAnimated", &json!([])).as_bool().unwrap_or(false);
            return simple("setEmojiAnimated", &json!([!animated]));
        }
        if let Some(icon) = key.strip_prefix(APP_ICON_PREFIX) {
            return simple("setAppIcon", &json!([icon]));
        }
        if let Some(pack) = key.strip_prefix("icon-pack.") {
            return simple("selectIconPack", &json!([pack]));
        }
        if let Some(theme) = key.strip_prefix("theme.") {
            return simple("applyTheme", &json!([theme]));
        }
    }
    if route == "marketplace" || route == "updates" {
        install_from_key(key, &changed);
    }
    None
}

pub fn configure(host: Option<HostCall>, settings: *const AstralSettingsHandle, version: String) {
    let mut platform = PLATFORM.write().unwrap_or_else(|poisoned| poisoned.into_inner());
    *platform = Platform {
        host,
        settings: settings as usize,
        version,
    };
}

fn json_string(value: &Value) -> AstralRustString {
    AstralRustString::new(value.to_string().into_bytes())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_settings_pages_configure(
    host: Option<HostCall>,
    settings: *const AstralSettingsHandle,
    version: *const c_char,
    version_length: usize,
) {
    configure(host, settings, text(version, version_length));
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_settings_pages_category() -> AstralRustString {
    json_string(&category())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_settings_pages_model(route: *const c_char, length: usize) -> AstralRustString {
    json_string(&model(&text(route, length)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_settings_pages_action(
    route: *const c_char,
    route_length: usize,
    key: *const c_char,
    key_length: usize,
    changed: AstralCallback,
) -> AstralRustString {
    optional(action(&text(route, route_length), &text(key, key_length), adopt_changed(changed)).map(String::into_bytes))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_settings_pages_version_is_newer(
    candidate: *const c_char,
    candidate_length: usize,
    installed: *const c_char,
    installed_length: usize,
) -> c_int {
    c_int::from(version_is_newer(
        &text(candidate, candidate_length),
        &text(installed, installed_length),
    ))
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_settings_pages_catalog_refresh_due(now: i64, last: i64) -> c_int {
    c_int::from(catalog_refresh_due(now, last))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_settings_pages_refresh_catalog_if_due(now: i64, force: c_int, changed: AstralCallback) {
    refresh_catalog_if_due(now, force != 0, adopt_changed(changed));
}
