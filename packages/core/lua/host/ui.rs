use crate::lua::enums::{Event, OptionType, Target};
use crate::plugins::capability::Capability;
use core::ffi::c_int;
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

use crate::lua::api::{self as lua, LuaState, NOREF};
use crate::lua::host::api::{Outcome, check_bytes, check_integer, check_text, check_type, message_argument, opt_text, require_permission};
use crate::lua::host::{self as host, Platform, Plugin, SettingsButton, Switch, TextSetting};
use crate::lua::json::to_json;

pub fn snowflake_text(value: &str) -> bool {
    !value.is_empty() && value.len() <= 20 && value.bytes().all(|byte| byte.is_ascii_digit())
}

const DISCORD_BODY_LIMIT: usize = 256 << 10;
const MESSAGE_ACTION_LABEL_LIMIT: usize = 63;

fn me(state: *mut LuaState) -> &'static mut Plugin {
    unsafe { &mut *host::plugin_of(state) }
}

fn clip(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_owned();
    }
    let mut end = limit;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

unsafe fn field_string(state: *mut LuaState, index: c_int, name: &str) -> String {
    unsafe {
        lua::get_field(state, index, name);
        let value = if lua::lua_isstring(state, -1) != 0 {
            String::from_utf8_lossy(lua::to_bytes(state, -1).unwrap_or_default()).into_owned()
        } else {
            String::new()
        };
        lua::pop(state, 1);
        value
    }
}

unsafe fn field_number(state: *mut LuaState, index: c_int, name: &str, fallback: f64) -> f64 {
    unsafe {
        lua::get_field(state, index, name);
        let value = if lua::lua_isnumber(state, -1) != 0 {
            lua::to_number(state, -1)
        } else {
            fallback
        };
        lua::pop(state, 1);
        value
    }
}

unsafe fn push_number(state: *mut LuaState, value: f64) {
    if value == value.floor() && value.abs() < 9.0e15 {
        unsafe { lua::lua_pushinteger(state, value as i64) };
    } else {
        unsafe { lua::lua_pushnumber(state, value) };
    }
}

unsafe fn reference_at(state: *mut LuaState, index: c_int) -> c_int {
    unsafe {
        lua::lua_pushvalue(state, index);
        lua::reference(state)
    }
}

unsafe fn push_flag(state: *mut LuaState, flag: bool) -> Outcome {
    unsafe { lua::lua_pushboolean(state, c_int::from(flag)) };
    Ok(1)
}

pub unsafe fn log(state: *mut LuaState) -> Outcome {
    let top = unsafe { lua::lua_gettop(state) };
    let mut parts = Vec::new();
    for index in 1..=top {
        let mut length = 0;
        let text = unsafe { lua::luaL_tolstring(state, index, &mut length) };
        parts.push(String::from_utf8_lossy(unsafe { core::slice::from_raw_parts(text.cast::<u8>(), length) }).into_owned());
        unsafe { lua::pop(state, 1) };
    }
    let line = format!("{}: {}", me(state).identifier(), parts.join(" "));
    crate::support::log::log(crate::support::log::INFO, "plugin", &line);
    host::developer_log(&line);
    Ok(0)
}

pub unsafe fn translate(state: *mut LuaState) -> Outcome {
    let key = unsafe { check_text(state, 1) }?;
    let mut values = Vec::new();
    if unsafe { lua::is_table(state, 2) } {
        unsafe { lua::lua_pushnil(state) };
        while unsafe { lua::lua_next(state, 2) } != 0 {
            if unsafe { lua::lua_type(state, -2) } == lua::TSTRING {
                let name = String::from_utf8_lossy(unsafe { lua::to_bytes(state, -2) }.unwrap_or_default()).into_owned();
                let mut length = 0;
                let text = unsafe { lua::luaL_tolstring(state, -1, &mut length) };
                values.push((
                    name,
                    String::from_utf8_lossy(unsafe { core::slice::from_raw_parts(text.cast::<u8>(), length) }).into_owned(),
                ));
                unsafe { lua::pop(state, 1) };
            }
            unsafe { lua::pop(state, 1) };
        }
    }
    let text = me(state).translate(&key, &values);
    unsafe { lua::push_str(state, &text) };
    Ok(1)
}

const MAX_TIMERS: usize = 64;
const MAX_TIMER_DELAY: i64 = 24 * 60 * 60 * 1000;
const MIN_TIMER_INTERVAL: i64 = 16;

unsafe fn add_timer(state: *mut LuaState, repeating: bool) -> Outcome {
    let plugin = me(state);
    let delay = unsafe { check_integer(state, 1) }?;
    unsafe { check_type(state, 2, lua::TFUNCTION) }?;
    if !(0..=MAX_TIMER_DELAY).contains(&delay) {
        return Err("astral.time: delay is 0 to 86400000 milliseconds".into());
    }
    if plugin.timers.len() >= MAX_TIMERS {
        return Err("astral.time: at most 64 timers per plugin".into());
    }
    let reference = unsafe { reference_at(state, 2) };
    let identifier = host::NEXT_REQUEST.fetch_add(1, Ordering::Relaxed);
    let interval = if repeating { delay.max(MIN_TIMER_INTERVAL) } else { 0 };
    plugin.timers.insert(identifier, crate::lua::host::Timer { reference, interval });
    crate::lua::host::timers::schedule(identifier, if repeating { interval } else { delay });
    unsafe { lua::lua_pushinteger(state, identifier as i64) };
    Ok(1)
}

pub unsafe fn time_after(state: *mut LuaState) -> Outcome {
    unsafe { add_timer(state, false) }
}

pub unsafe fn time_every(state: *mut LuaState) -> Outcome {
    unsafe { add_timer(state, true) }
}

pub unsafe fn time_cancel(state: *mut LuaState) -> Outcome {
    let identifier = unsafe { check_integer(state, 1) }? as u64;
    if let Some(timer) = me(state).timers.remove(&identifier) {
        unsafe { lua::release(state, timer.reference) };
    }
    Ok(0)
}

unsafe fn option_strings(state: *mut LuaState, index: c_int) -> (String, String, String) {
    if !unsafe { lua::is_table(state, index) } {
        return Default::default();
    }
    unsafe {
        (
            field_string(state, index, "subtitle"),
            field_string(state, index, "section"),
            field_string(state, index, "icon"),
        )
    }
}

pub unsafe fn settings_switch(state: *mut LuaState) -> Outcome {
    let plugin = me(state);
    let key = unsafe { check_text(state, 1) }?;
    let title = unsafe { check_text(state, 2) }?;
    let fallback = unsafe { lua::lua_toboolean(state, 3) } != 0;
    let (subtitle, section, icon) = unsafe { option_strings(state, 4) };
    if key.is_empty() || key.len() > 48 || plugin.switches.len() >= 32 {
        return Err("astral.settings.switch: invalid key".into());
    }
    if !plugin.switches.iter().any(|entry| entry.key == key) {
        let order = plugin.setting_count;
        plugin.setting_count += 1;
        plugin.switches.push(Switch {
            key: key.clone(),
            title: clip(&title, 64),
            subtitle: clip(&subtitle, 120),
            section: clip(&section, 48),
            fallback,
            maximum: 100.0,
            step: 1.0,
            icon: clip(&icon, 64),
            order,
            ..Switch::default()
        });
    }
    let value = host::stored_bool(&host::setting_key(plugin, &key), fallback);
    unsafe { push_flag(state, value) }
}

pub unsafe fn settings_button(state: *mut LuaState) -> Outcome {
    let plugin = me(state);
    let key = unsafe { check_text(state, 1) }?;
    let title = unsafe { check_text(state, 2) }?;
    unsafe { check_type(state, 3, lua::TFUNCTION) }?;
    if key.is_empty() || key.len() > 48 {
        return Err("astral.settings.button: invalid key".into());
    }
    let position = match plugin.settings_buttons.iter().position(|entry| entry.key == key) {
        Some(position) => {
            unsafe { lua::release(state, plugin.settings_buttons[position].reference) };
            position
        }
        None => {
            if plugin.settings_buttons.len() >= 16 {
                return Err("astral.settings.button: at most 16 buttons per plugin".into());
            }
            let order = plugin.setting_count;
            plugin.setting_count += 1;
            plugin.settings_buttons.push(SettingsButton {
                key,
                title: String::new(),
                subtitle: String::new(),
                section: String::new(),
                icon: String::new(),
                detail: String::new(),
                order,
                reference: NOREF,
            });
            plugin.settings_buttons.len() - 1
        }
    };
    let button = &mut plugin.settings_buttons[position];
    button.title = clip(&title, 64);
    if unsafe { lua::is_table(state, 4) } {
        unsafe {
            button.subtitle = clip(&field_string(state, 4, "subtitle"), 120);
            button.section = clip(&field_string(state, 4, "section"), 48);
            button.icon = clip(&field_string(state, 4, "icon"), 64);
            button.detail = clip(&field_string(state, 4, "detail"), 32);
        }
    }
    button.reference = unsafe { reference_at(state, 3) };
    Ok(0)
}

pub unsafe fn settings_choice(state: *mut LuaState) -> Outcome {
    let plugin = me(state);
    let key = unsafe { check_text(state, 1) }?;
    let title = unsafe { check_text(state, 2) }?;
    unsafe { check_type(state, 3, lua::TTABLE) }?;
    let fallback = unsafe { check_text(state, 4) }?;
    let mut entry = Switch {
        key: key.clone(),
        title: clip(&title, 64),
        fallback_choice: fallback.clone(),
        maximum: 100.0,
        step: 1.0,
        ..Switch::default()
    };
    if unsafe { lua::is_table(state, 5) } {
        let (subtitle, section, icon) = unsafe { option_strings(state, 5) };
        entry.subtitle = clip(&subtitle, 120);
        entry.section = clip(&section, 48);
        entry.icon = clip(&icon, 64);
    }
    let count = unsafe { lua::luaL_len(state, 3) };
    if count <= 0 || count > 64 {
        return Err("astral.settings.choice: provide between 1 and 64 choices".into());
    }
    for index in 1..=count {
        unsafe { lua::lua_rawgeti(state, 3, index) };
        if unsafe { lua::is_table(state, -1) } {
            let option = unsafe {
                crate::lua::host::Option_ {
                    value: field_string(state, -1, "value"),
                    title: field_string(state, -1, "title"),
                    subtitle: field_string(state, -1, "subtitle"),
                }
            };
            if option.value.len() > 128 || option.title.len() > 96 || option.subtitle.len() > 120 {
                unsafe { lua::pop(state, 1) };
                return Err("astral.settings.choice: option text exceeds its limit".into());
            }
            if !option.value.is_empty() {
                entry.options.push(option);
            }
        }
        unsafe { lua::pop(state, 1) };
    }
    if key.is_empty() || key.len() > 48 || entry.options.is_empty() || plugin.switches.len() >= 32 {
        return Err("astral.settings.choice: invalid choice".into());
    }
    if !plugin.switches.iter().any(|existing| existing.key == key) {
        entry.order = plugin.setting_count;
        plugin.setting_count += 1;
        plugin.switches.push(entry);
    }
    let value = host::stored_string(&host::setting_key(plugin, &key), &fallback);
    unsafe { lua::push_str(state, &value) };
    Ok(1)
}

pub unsafe fn settings_number(state: *mut LuaState) -> Outcome {
    let plugin = me(state);
    let key = unsafe { check_text(state, 1) }?;
    let title = unsafe { check_text(state, 2) }?;
    unsafe { check_type(state, 3, lua::TTABLE) }?;
    let mut entry = Switch {
        key: key.clone(),
        title: clip(&title, 64),
        number: true,
        ..Switch::default()
    };
    unsafe {
        entry.minimum = field_number(state, 3, "min", 0.0);
        entry.maximum = field_number(state, 3, "max", 100.0);
        entry.step = field_number(state, 3, "step", 1.0);
        entry.subtitle = clip(&field_string(state, 3, "subtitle"), 120);
        entry.section = clip(&field_string(state, 3, "section"), 48);
        entry.icon = clip(&field_string(state, 3, "icon"), 64);
    }
    let valid = !key.is_empty()
        && key.len() <= 48
        && plugin.switches.len() < 32
        && entry.minimum.is_finite()
        && entry.maximum.is_finite()
        && entry.maximum > entry.minimum
        && entry.step > 0.0
        && (entry.maximum - entry.minimum) / entry.step <= 1000.0;
    if !valid {
        return Err("astral.settings.number: invalid number".into());
    }
    entry.fallback_number = host::stepped_number(&entry, unsafe { field_number(state, 3, "default", entry.minimum) });
    let stored = host::stepped_number(&entry, host::stored_number(&host::setting_key(plugin, &key), entry.fallback_number));
    if !plugin.switches.iter().any(|existing| existing.key == key) {
        entry.order = plugin.setting_count;
        plugin.setting_count += 1;
        plugin.switches.push(entry);
    }
    unsafe { push_number(state, stored) };
    Ok(1)
}

fn is_color_text(value: &str) -> bool {
    value.is_empty() || crate::render::swatch::parse_color(value.as_bytes()).is_some()
}

pub unsafe fn settings_text(state: *mut LuaState) -> Outcome {
    let plugin = me(state);
    let key = unsafe { check_text(state, 1) }?;
    let title = unsafe { check_text(state, 2) }?;
    let fallback = unsafe { check_text(state, 3) }?;
    const MAXIMUM: usize = 16384;
    let mut entry = TextSetting {
        key: key.clone(),
        title: clip(&title, 64),
        fallback: fallback.clone(),
        maximum_length: MAXIMUM,
        ..TextSetting::default()
    };
    if unsafe { lua::is_table(state, 4) } {
        let (subtitle, section, icon) = unsafe { option_strings(state, 4) };
        entry.subtitle = clip(&subtitle, 120);
        entry.section = clip(&section, 48);
        entry.icon = clip(&icon, 64);
        unsafe { lua::get_field(state, 4, "color") };
        entry.color = unsafe { lua::lua_toboolean(state, -1) } != 0;
        unsafe { lua::pop(state, 1) };
        let requested = unsafe { field_number(state, 4, "max_length", MAXIMUM as f64) };
        if !requested.is_finite() || requested < 1.0 || requested > MAXIMUM as f64 || requested.floor() != requested {
            return Err("astral.settings.text: max_length must be between 1 and 16384".into());
        }
        entry.maximum_length = requested as usize;
    }
    if entry.color {
        entry.maximum_length = 9;
    }
    if key.is_empty() || key.len() > 48 || fallback.len() > entry.maximum_length || plugin.text_settings.len() >= 16 {
        return Err("astral.settings.text: invalid text setting".into());
    }
    if entry.color && !is_color_text(&fallback) {
        return Err("astral.settings.color: default must be empty, #RRGGBB or #AARRGGBB".into());
    }
    let (maximum, color) = (entry.maximum_length, entry.color);
    if !plugin.text_settings.iter().any(|existing| existing.key == key) && !plugin.switches.iter().any(|existing| existing.key == key) {
        entry.order = plugin.setting_count;
        plugin.setting_count += 1;
        plugin.text_settings.push(entry);
    }
    let mut value = host::stored_string(&host::setting_key(plugin, &key), &fallback);
    if value.len() > maximum || (color && !is_color_text(&value)) {
        value = fallback;
    }
    unsafe { lua::push_str(state, &value) };
    Ok(1)
}

pub unsafe fn settings_get(state: *mut LuaState) -> Outcome {
    let key = unsafe { check_text(state, 1) }?;
    match host::stored(&host::setting_key(me(state), &key)) {
        Some(Value::Bool(flag)) => unsafe { lua::lua_pushboolean(state, c_int::from(flag)) },
        Some(Value::Number(number)) if number.is_i64() => unsafe { lua::lua_pushinteger(state, number.as_i64().unwrap_or(0)) },
        Some(Value::Number(number)) => unsafe { lua::lua_pushnumber(state, number.as_f64().unwrap_or(0.0)) },
        Some(Value::String(text)) => unsafe { lua::push_str(state, &text) },
        _ => unsafe { lua::lua_pushvalue(state, 2) },
    }
    Ok(1)
}

pub unsafe fn settings_set(state: *mut LuaState) -> Outcome {
    let key = host::setting_key(me(state), &unsafe { check_text(state, 1) }?);
    let value = match unsafe { lua::lua_type(state, 2) } {
        lua::TBOOLEAN => json!(unsafe { lua::lua_toboolean(state, 2) } != 0),
        lua::TNUMBER if unsafe { lua::lua_isinteger(state, 2) } != 0 => json!(unsafe { lua::to_integer(state, 2) }),
        lua::TNUMBER => json!(unsafe { lua::to_number(state, 2) }),
        lua::TSTRING => json!(String::from_utf8_lossy(unsafe { lua::to_bytes(state, 2) }.unwrap_or_default())),
        _ => return Err("astral.settings.set: unsupported value".into()),
    };
    host::store(&key, value);
    host::refresh_decorations();
    Ok(0)
}

fn notify_setting(plugin: *mut Plugin, key: &str, push_value: impl FnOnce(*mut LuaState)) {
    let state = unsafe { (*plugin).state() };
    if state.is_null() {
        return;
    }
    unsafe {
        lua::lua_getglobal(state, c"astral".as_ptr());
        lua::get_field(state, -1, "on_setting");
        lua::remove(state, -2);
    }
    if unsafe { lua::is_function(state, -1) } {
        unsafe { lua::push_str(state, key) };
        push_value(state);
        host::call(plugin, 2, 0, "on_setting");
    } else {
        unsafe { lua::pop(state, 1) };
    }
    host::refresh_decorations();
}

pub fn set_choice(identifier: &str, key: &str, value: &str) {
    let _held = host::lock();
    let Some(plugin) = host::find(identifier) else { return };
    host::store(&host::setting_key(unsafe { &*plugin }, key), json!(value));
    notify_setting(plugin, key, |state| unsafe { lua::push_str(state, value) });
}

pub fn set_number(identifier: &str, key: &str, value: f64) {
    let _held = host::lock();
    let Some(plugin) = host::find(identifier) else { return };
    let Some(entry) = (unsafe { &*plugin })
        .switches
        .iter()
        .rev()
        .find(|candidate| candidate.number && candidate.key == key)
        .cloned()
    else {
        return;
    };
    let stepped = host::stepped_number(&entry, value);
    let setting = host::setting_key(unsafe { &*plugin }, key);
    if host::stored_number(&setting, entry.fallback_number) == stepped {
        return;
    }
    host::store(&setting, json!(stepped));
    notify_setting(plugin, key, |state| unsafe { push_number(state, stepped) });
}

pub fn set_switch(identifier: &str, key: &str, on: bool) {
    let _held = host::lock();
    let Some(plugin) = host::find(identifier) else { return };
    host::store(&host::setting_key(unsafe { &*plugin }, key), json!(on));
    notify_setting(plugin, key, |state| unsafe { lua::lua_pushboolean(state, c_int::from(on)) });
}

pub fn press_settings_button(identifier: &str, key: &str) {
    let _held = host::lock();
    let Some(plugin) = host::find(identifier) else { return };
    if !unsafe { (*plugin).running() } {
        return;
    }
    let Some(reference) = (unsafe { &*plugin })
        .settings_buttons
        .iter()
        .find(|button| button.key == key && button.reference != NOREF)
        .map(|button| button.reference)
    else {
        return;
    };
    unsafe { host::push_callback(plugin, reference, false) };
    host::call_or_stop(plugin, 0, 0, "settings button");
}

pub fn set_text(identifier: &str, key: &str, value: &str) {
    let _held = host::lock();
    let Some(plugin) = host::find(identifier) else { return };
    if unsafe { (*plugin).state.is_none() } {
        return;
    }
    let Some(setting) = (unsafe { &*plugin })
        .text_settings
        .iter()
        .rev()
        .find(|candidate| candidate.key == key)
        .cloned()
    else {
        return;
    };
    if value.len() > setting.maximum_length || (setting.color && !is_color_text(value)) {
        return;
    }
    let stored_key = host::setting_key(unsafe { &*plugin }, key);
    if host::stored_string(&stored_key, &setting.fallback) == value {
        return;
    }
    host::store(&stored_key, json!(value));
    notify_setting(plugin, key, |state| unsafe { lua::push_str(state, value) });
}

pub unsafe fn show_alert(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, Capability::UiDialogs) }?;
    let (title, message) = (unsafe { check_text(state, 1) }?, unsafe { check_text(state, 2) }?);
    if title.len() > 120 || message.len() > 2048 {
        return Err("astral.ui.alert: text exceeds its limit".into());
    }
    match host::platform("showAlert", &json!([title, message])) {
        Platform::Missing => Err("astral.ui.alert: dialogs are unavailable on this build".into()),
        Platform::Threw => Err("astral.ui.alert: could not show the dialog".into()),
        Platform::Value(_) => Ok(0),
    }
}

unsafe fn confirm_text(state: *mut LuaState, field: &str, limit: usize, fallback: &str) -> Result<String, String> {
    unsafe { lua::get_field(state, 1, field) };
    let value = if unsafe { lua::lua_isstring(state, -1) } != 0 {
        String::from_utf8_lossy(unsafe { lua::to_bytes(state, -1) }.unwrap_or_default()).into_owned()
    } else {
        fallback.to_owned()
    };
    unsafe { lua::pop(state, 1) };
    if value.len() > limit {
        return Err(format!("astral.ui.confirm: {field} exceeds {limit} bytes"));
    }
    Ok(value)
}

fn start_request(
    table: impl FnOnce(&mut Plugin) -> &mut std::collections::BTreeMap<u64, c_int>,
    plugin: &mut Plugin,
    state: *mut LuaState,
    index: c_int,
) -> u64 {
    let request = host::next_request();
    let reference = unsafe { reference_at(state, index) };
    table(plugin).insert(request, reference);
    request
}

fn abandon_request(
    table: impl FnOnce(&mut Plugin) -> &mut std::collections::BTreeMap<u64, c_int>,
    plugin: &mut Plugin,
    state: *mut LuaState,
    request: u64,
) {
    if let Some(reference) = table(plugin).remove(&request) {
        unsafe { lua::release(state, reference) };
    }
}

unsafe fn confirm_details(state: *mut LuaState) -> Result<Value, String> {
    unsafe { lua::get_field(state, 1, "details") };
    let details = if unsafe { lua::is_table(state, -1) } {
        unsafe { to_json(state, -1, 0) }.unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    unsafe { lua::pop(state, 1) };
    let mut rows = Vec::new();
    for row in details.as_array().into_iter().flatten().take(6) {
        let text = row.get("text").and_then(Value::as_str).unwrap_or_default();
        if text.is_empty() || text.len() > 160 {
            return Err("astral.ui.confirm: each detail needs text of at most 160 bytes".into());
        }
        let icon = row
            .get("icon")
            .and_then(Value::as_str)
            .map(crate::settings::pages::mana_asset)
            .unwrap_or_default();
        rows.push(json!({"text": text, "icon": icon}));
    }
    Ok(Value::Array(rows))
}

pub unsafe fn confirm(state: *mut LuaState) -> Outcome {
    let plugin = me(state);
    unsafe { require_permission(state, Capability::UiDialogs) }?;
    unsafe { check_type(state, 1, lua::TTABLE) }?;
    unsafe { check_type(state, 2, lua::TFUNCTION) }?;
    let mut spec = unsafe {
        json!({
            "title": confirm_text(state, "title", 120, "")?,
            "message": confirm_text(state, "message", 2048, "")?,
            "image": confirm_text(state, "image", 2048, "")?,
            "confirm": confirm_text(state, "confirm", 40, "OK")?,
            "cancel": confirm_text(state, "cancel", 40, "Cancel")?,
            "subtitle": confirm_text(state, "subtitle", 160, "")?,
            "icon": confirm_text(state, "icon", 256, "")?,
        })
    };
    unsafe { lua::get_field(state, 1, "destructive") };
    spec["destructive"] = json!(unsafe { lua::lua_toboolean(state, -1) } != 0);
    unsafe { lua::pop(state, 1) };
    let icon = spec["icon"].as_str().unwrap_or_default().to_owned();
    if !icon.is_empty() {
        spec["icon"] = json!(crate::settings::pages::mana_asset(&icon));
    }
    spec["details"] = unsafe { confirm_details(state) }?;
    let image = spec["image"].as_str().unwrap_or_default().to_owned();
    if spec["title"].as_str().unwrap_or_default().is_empty() {
        return Err("astral.ui.confirm: give a title".into());
    }
    if !image.is_empty() && !image.starts_with("https://") {
        return Err("astral.ui.confirm: image must be an https URL".into());
    }
    if plugin.confirm_requests.len() >= 4 {
        return Err("astral.ui.confirm: at most 4 open dialogs per plugin".into());
    }
    if !host::platform_has("confirm") {
        return Err("astral.ui.confirm: dialogs are unavailable on this build".into());
    }
    let request = start_request(|plugin| &mut plugin.confirm_requests, plugin, state, 2);
    if !host::platform_flag("confirm", &json!([request, spec])) {
        abandon_request(|plugin| &mut plugin.confirm_requests, me(state), state, request);
        return Err("astral.ui.confirm: could not show the dialog".into());
    }
    unsafe { lua::lua_pushinteger(state, request as i64) };
    Ok(1)
}

unsafe fn sheet_spec(state: *mut LuaState, index: c_int, function: &str) -> Result<Value, String> {
    unsafe { check_type(state, index, lua::TTABLE) }?;
    let Some(spec) = (unsafe { to_json(state, index, 0) }).filter(Value::is_object) else {
        return Err(format!("{function}: give a sheet table"));
    };
    if spec.to_string().len() > 256 * 1024 {
        return Err(format!("{function}: the sheet exceeds 256 KiB"));
    }
    Ok(spec)
}

unsafe fn sheet_id(state: *mut LuaState, function: &str) -> Result<u64, String> {
    let value = unsafe { check_integer(state, 1) }?;
    let sheet = if value > 0 { value as u64 } else { 0 };
    if !me(state).sheets.contains_key(&sheet) {
        return Err(format!("{function}: that sheet is not open"));
    }
    Ok(sheet)
}

pub unsafe fn open_sheet(state: *mut LuaState) -> Outcome {
    let plugin = me(state);
    unsafe { require_permission(state, Capability::UiDialogs) }?;
    let spec = unsafe { sheet_spec(state, 1, "astral.ui.sheet") }?;
    unsafe { check_type(state, 2, lua::TFUNCTION) }?;
    if plugin.sheets.len() >= 4 {
        return Err("astral.ui.sheet: at most 4 open sheets per plugin".into());
    }
    if !host::platform_has("openSheet") {
        return Err("astral.ui.sheet: sheets are unavailable on this build".into());
    }
    let sheet = start_request(|plugin| &mut plugin.sheets, plugin, state, 2);
    if !host::platform_flag("openSheet", &json!([sheet, spec])) {
        abandon_request(|plugin| &mut plugin.sheets, me(state), state, sheet);
        return Err("astral.ui.sheet: could not show the sheet".into());
    }
    unsafe { lua::lua_pushinteger(state, sheet as i64) };
    Ok(1)
}

pub unsafe fn update_sheet(state: *mut LuaState) -> Outcome {
    let sheet = unsafe { sheet_id(state, "astral.ui.sheet_update") }?;
    let spec = unsafe { sheet_spec(state, 2, "astral.ui.sheet_update") }?;
    host::platform("updateSheet", &json!([sheet, spec]));
    Ok(0)
}

pub unsafe fn close_sheet(state: *mut LuaState) -> Outcome {
    let sheet = unsafe { sheet_id(state, "astral.ui.sheet_close") }?;
    host::platform("closeSheet", &json!([sheet]));
    Ok(0)
}

unsafe fn toast_spec(state: *mut LuaState, index: c_int, function: &str) -> Result<Value, String> {
    unsafe { check_type(state, index, lua::TTABLE) }?;
    let Some(spec) = (unsafe { to_json(state, index, 0) }).filter(Value::is_object) else {
        return Err(format!("{function}: give a toast table"));
    };
    if spec.to_string().len() > 8 * 1024 {
        return Err(format!("{function}: the toast exceeds 8 KiB"));
    }
    Ok(spec)
}

unsafe fn toast_id(state: *mut LuaState) -> Result<Option<u64>, String> {
    let value = unsafe { check_integer(state, 1) }?;
    let toast = if value > 0 { value as u64 } else { 0 };
    Ok(me(state).toasts.contains_key(&toast).then_some(toast))
}

pub unsafe fn show_toast(state: *mut LuaState) -> Outcome {
    let plugin = me(state);
    unsafe { require_permission(state, Capability::UiToasts) }?;
    let spec = unsafe { toast_spec(state, 1, "astral.toasts.show") }?;
    unsafe { check_type(state, 2, lua::TFUNCTION) }?;
    if plugin.toasts.len() >= 8 {
        return Err("astral.toasts.show: at most 8 toasts per plugin".into());
    }
    let toast = start_request(|plugin| &mut plugin.toasts, plugin, state, 2);
    host::platform("showToast", &json!([toast, spec]));
    unsafe { lua::lua_pushinteger(state, toast as i64) };
    Ok(1)
}

pub unsafe fn update_toast(state: *mut LuaState) -> Outcome {
    let Some(toast) = (unsafe { toast_id(state) })? else {
        return Ok(0);
    };
    let spec = unsafe { toast_spec(state, 2, "astral.toasts.update") }?;
    host::platform("showToast", &json!([toast, spec]));
    Ok(0)
}

pub unsafe fn close_toast(state: *mut LuaState) -> Outcome {
    let Some(toast) = (unsafe { toast_id(state) })? else {
        return Ok(0);
    };
    host::platform("closeToast", &json!([toast]));
    Ok(0)
}

pub unsafe fn press_node(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, Capability::UiPress) }?;
    let raw = unsafe { check_bytes(state, 1) }?;
    if raw.is_empty() || raw.len() > 128 {
        return Err("astral.ui.press: identifiers are 1 to 128 characters".into());
    }
    let mut wanted = String::from_utf8_lossy(raw).into_owned();
    let prefix = wanted.ends_with('*');
    if prefix {
        wanted.pop();
    }
    if wanted.bytes().any(|byte| byte.is_ascii_control()) {
        return Err("astral.ui.press: identifier has control characters".into());
    }
    unsafe { push_flag(state, host::platform_flag("pressNode", &json!([wanted, prefix]))) }
}

pub unsafe fn register_image_rewrite(state: *mut LuaState) -> Outcome {
    let plugin = me(state);
    unsafe { require_permission(state, Capability::UiImages) }?;
    let prefix = unsafe { check_text(state, 1) }?;
    unsafe { check_type(state, 2, lua::TFUNCTION) }?;
    if !prefix.starts_with("http") {
        return Err("astral.ui.image_rewrite: the prefix must start with http".into());
    }
    if plugin.image_rewriters.len() >= 16 {
        return Err("astral.ui.image_rewrite: at most 16 rewriters per plugin".into());
    }
    let reference = unsafe { reference_at(state, 2) };
    plugin.image_rewriters.push((prefix, reference));
    Ok(0)
}

pub unsafe fn register_text_transform(state: *mut LuaState) -> Outcome {
    let plugin = me(state);
    unsafe { require_permission(state, Capability::UiText) }?;
    unsafe { check_type(state, 1, lua::TFUNCTION) }?;
    if plugin.text_transforms.len() >= 8 {
        return Err("astral.ui.text_transform: at most 8 transforms per plugin".into());
    }
    let reference = unsafe { reference_at(state, 1) };
    plugin.text_transforms.push(reference);
    host::refresh_decorations();
    Ok(0)
}

fn option_type(kind: &str) -> i64 {
    OptionType::parse(kind).map_or(0, OptionType::discord_type)
}

fn command_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 32
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_')
}

unsafe fn string_field_or(state: *mut LuaState, index: c_int, name: &str, fallback: &str) -> String {
    unsafe { lua::get_field(state, index, name) };
    let value = if unsafe { lua::lua_isstring(state, -1) } != 0 {
        String::from_utf8_lossy(unsafe { lua::to_bytes(state, -1) }.unwrap_or_default()).into_owned()
    } else {
        fallback.to_owned()
    };
    unsafe { lua::pop(state, 1) };
    value
}

pub unsafe fn register_command(state: *mut LuaState) -> Outcome {
    let plugin = me(state);
    unsafe { require_permission(state, Capability::Commands) }?;
    unsafe { check_type(state, 1, lua::TTABLE) }?;
    if plugin.commands.len() >= 16 {
        return Err("astral.commands.register: at most 16 commands per plugin".into());
    }
    let name = unsafe { string_field_or(state, 1, "name", "") };
    let description = unsafe { string_field_or(state, 1, "description", "") };
    if !command_name(&name) {
        return Err("astral.commands.register: names are 1 to 32 lower-case letters, digits, - or _".into());
    }
    if description.is_empty() || description.len() > 100 {
        return Err("astral.commands.register: descriptions are 1 to 100 characters".into());
    }
    let mut options = Vec::new();
    unsafe { lua::get_field(state, 1, "options") };
    if unsafe { lua::is_table(state, -1) } {
        let count = unsafe { lua::luaL_len(state, -1) };
        if count > 25 {
            return Err("astral.commands.register: at most 25 options".into());
        }
        for index in 1..=count {
            unsafe { lua::lua_rawgeti(state, -1, index) };
            let option_name = unsafe { string_field_or(state, -1, "name", "") };
            let option_description = unsafe { string_field_or(state, -1, "description", &option_name) };
            let kind = option_type(&unsafe { string_field_or(state, -1, "type", "string") });
            unsafe { lua::get_field(state, -1, "required") };
            let required = unsafe { lua::lua_toboolean(state, -1) } != 0;
            unsafe { lua::pop(state, 2) };
            if !command_name(&option_name) || kind == 0 {
                return Err(format!("astral.commands.register: option {index} needs a valid name and type"));
            }
            options.push(json!({"name": option_name, "description": clip(&option_description, 100), "type": kind, "required": required}));
        }
    }
    unsafe { lua::pop(state, 1) };
    unsafe { lua::get_field(state, 1, "execute") };
    if !unsafe { lua::is_function(state, -1) } {
        return Err("astral.commands.register: execute must be a function".into());
    }
    let reference = unsafe { lua::reference(state) };
    let command = crate::lua::host::Command {
        name,
        description,
        options: Value::Array(options),
        reference,
    };
    match plugin.commands.iter_mut().find(|existing| existing.name == command.name) {
        Some(existing) => {
            unsafe { lua::release(state, existing.reference) };
            *existing = command;
        }
        None => plugin.commands.push(command),
    }
    Ok(0)
}

pub unsafe fn unregister_command(state: *mut LuaState) -> Outcome {
    let plugin = me(state);
    unsafe { require_permission(state, Capability::Commands) }?;
    let name = unsafe { check_text(state, 1) }?;
    if let Some(index) = plugin.commands.iter().position(|command| command.name == name) {
        let command = plugin.commands.remove(index);
        unsafe { lua::release(state, command.reference) };
    }
    Ok(0)
}

pub unsafe fn render_image(state: *mut LuaState) -> Outcome {
    let spec = if unsafe { lua::is_table(state, 1) } {
        unsafe { to_json(state, 1, 0) }.filter(Value::is_object)
    } else {
        None
    };
    let Some(spec) = spec else {
        return Err("astral.image.render: give a layout table".into());
    };
    let number = |name: &str, fallback: f64| spec.get(name).and_then(Value::as_f64).unwrap_or(fallback);
    let (width, height, scale) = (number("width", 0.0), number("height", 0.0), number("scale", 2.0));
    if !(1.0..=2048.0).contains(&width) || !(1.0..=4096.0).contains(&height) || !(1.0..=3.0).contains(&scale) {
        return Err("astral.image.render: width up to 2048, height up to 4096, scale 1 to 3".into());
    }
    if !spec.get("items").and_then(Value::as_array).is_some_and(|items| items.len() <= 2000) {
        return Err("astral.image.render: items must be a list of at most 2000 shapes".into());
    }
    let Some(Value::String(png)) = host::platform_value("renderImage", &json!([spec])) else {
        return Ok(0);
    };
    let Some(bytes) = crate::lua::host::install::decode_base64(&png) else {
        return Ok(0);
    };
    unsafe { lua::push_bytes(state, &bytes) };
    Ok(1)
}

pub unsafe fn pick_file(state: *mut LuaState) -> Outcome {
    let plugin = me(state);
    unsafe { require_permission(state, Capability::FilesRead) }?;
    unsafe { check_type(state, 1, lua::TTABLE) }?;
    unsafe { check_type(state, 2, lua::TFUNCTION) }?;
    let count = unsafe { lua::lua_rawlen(state, 1) };
    if count == 0 || count > 16 {
        return Err("astral.files.pick: provide between 1 and 16 file extensions".into());
    }
    let mut extensions: Vec<String> = Vec::new();
    for index in 1..=count {
        unsafe { lua::lua_rawgeti(state, 1, index as i64) };
        let extension = unsafe { check_text(state, -1) };
        unsafe { lua::pop(state, 1) };
        let mut extension = extension?;
        if extension.starts_with('.') {
            extension.remove(0);
        }
        if extension.is_empty()
            || extension.len() > 16
            || !extension
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'+' || byte == b'-')
        {
            return Err("astral.files.pick: invalid file extension".into());
        }
        let extension = extension.to_ascii_lowercase();
        if !extensions.contains(&extension) {
            extensions.push(extension);
        }
    }
    if !host::platform_has("pickFile") {
        return Err("astral.files.pick: file selection is unavailable on this build".into());
    }
    let request = start_request(|plugin| &mut plugin.file_picks, plugin, state, 2);
    if !host::platform_flag("pickFile", &json!([request, extensions])) {
        abandon_request(|plugin| &mut plugin.file_picks, me(state), state, request);
        return Err("astral.files.pick: could not open the file picker".into());
    }
    unsafe { lua::lua_pushinteger(state, request as i64) };
    Ok(1)
}

unsafe fn cancel_request(
    state: *mut LuaState,
    permission: Capability,
    table: impl FnOnce(&mut Plugin) -> &mut std::collections::BTreeMap<u64, c_int>,
    method: &str,
) -> Outcome {
    unsafe { require_permission(state, permission) }?;
    let value = unsafe { check_integer(state, 1) }?;
    if value <= 0 {
        return unsafe { push_flag(state, false) };
    }
    let request = value as u64;
    let Some(reference) = table(me(state)).remove(&request) else {
        return unsafe { push_flag(state, false) };
    };
    unsafe { lua::release(state, reference) };
    host::platform(method, &json!([request]));
    unsafe { push_flag(state, true) }
}

pub unsafe fn cancel_file_pick(state: *mut LuaState) -> Outcome {
    unsafe { cancel_request(state, Capability::FilesRead, |plugin| &mut plugin.file_picks, "cancelFilePick") }
}

fn valid_utf8(value: &[u8]) -> bool {
    std::str::from_utf8(value).is_ok()
}

fn authority(url: &str) -> &str {
    let rest = url.get(8..).unwrap_or_default();
    &rest[..rest.find(['/', '?', '#']).unwrap_or(rest.len())]
}

fn valid_authorization_url(url: &str, maximum: usize) -> bool {
    if url.len() > maximum
        || !url.starts_with("https://")
        || url.contains('\\')
        || crate::lua::host::http::contains_control(url)
        || !valid_utf8(url.as_bytes())
    {
        return false;
    }
    let authority = authority(url);
    !authority.is_empty() && !authority.contains('@')
}

fn hex_value(byte: u8) -> Option<u8> {
    (byte as char).to_digit(16).map(|digit| digit as u8)
}

fn percent_decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) = (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
        {
            decoded.push(high * 16 + low);
            index += 3;
            continue;
        }
        decoded.push(if bytes[index] == b'+' { b' ' } else { bytes[index] });
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn redirect_uri(url: &str) -> String {
    let Some(query) = url.find('?') else { return String::new() };
    let rest = &url[query + 1..];
    let parameters = &rest[..rest.find('#').unwrap_or(rest.len())];
    parameters
        .split('&')
        .find_map(|pair| pair.strip_prefix("redirect_uri="))
        .map(percent_decoded)
        .unwrap_or_default()
}

pub unsafe fn web_auth(state: *mut LuaState) -> Outcome {
    let plugin = me(state);
    unsafe { require_permission(state, Capability::UiWebAuth) }?;
    let url = unsafe { check_text(state, 1) }?;
    let callback = unsafe { check_text(state, 2) }?;
    unsafe { check_type(state, 3, lua::TFUNCTION) }?;
    if !valid_authorization_url(&url, 4096) || !valid_authorization_url(&callback, 1024) {
        return Err("astral.ui.web_auth: URLs must be valid HTTPS URLs without credentials".into());
    }
    let redirects = redirect_uri(&url).starts_with(&callback);
    if (authority(&url) != authority(&callback) && !redirects) || callback.contains(['?', '#']) {
        return Err("astral.ui.web_auth: callback must be a path on the authorization URL's HTTPS host".into());
    }
    if host::plugins()
        .into_iter()
        .any(|pending| unsafe { !(*pending).web_auth_requests.is_empty() })
    {
        return Err("astral.ui.web_auth: another authorization flow is already active".into());
    }
    if !host::platform_has("webAuth") {
        return Err("astral.ui.web_auth: browser authorization is unavailable on this client".into());
    }
    let request = start_request(|plugin| &mut plugin.web_auth_requests, plugin, state, 3);
    if !host::platform_flag("webAuth", &json!([request, url, callback])) {
        abandon_request(|plugin| &mut plugin.web_auth_requests, me(state), state, request);
        return Err("astral.ui.web_auth: could not open the authorization window".into());
    }
    unsafe { lua::lua_pushinteger(state, request as i64) };
    Ok(1)
}

pub unsafe fn cancel_web_auth(state: *mut LuaState) -> Outcome {
    unsafe {
        cancel_request(
            state,
            Capability::UiWebAuth,
            |plugin| &mut plugin.web_auth_requests,
            "cancelWebAuth",
        )
    }
}

const fn event_permission(event: Event) -> Capability {
    match event {
        Event::AppForeground | Event::AppBackground => Capability::EventsLifecycle,
        Event::MessageCreate => Capability::EventsMessages,
        Event::MessageFilter => Capability::EventsMessagesFilter,
        Event::Gateway => Capability::EventsGateway,
    }
}

pub unsafe fn subscribe_event(state: *mut LuaState) -> Outcome {
    let plugin = me(state);
    let name = unsafe { check_text(state, 1) }?;
    let Some(event) = Event::parse(&name) else {
        return Err(format!("astral.events.on: unsupported event '{name}'"));
    };
    unsafe { require_permission(state, event_permission(event)) }?;
    unsafe { check_type(state, 2, lua::TFUNCTION) }?;
    if plugin.event_callbacks.len() >= 32 {
        return Err("astral.events.on: at most 32 subscriptions per plugin".into());
    }
    let mut types = BTreeSet::new();
    if event == Event::Gateway && unsafe { lua::is_table(state, 3) } {
        let count = unsafe { lua::luaL_len(state, 3) };
        if count > 64 {
            return Err("astral.events.on: at most 64 gateway event types".into());
        }
        for index in 1..=count {
            unsafe { lua::lua_rawgeti(state, 3, index) };
            let kind = unsafe { lua::to_bytes(state, -1) }
                .filter(|kind| !kind.is_empty() && kind.len() <= 64)
                .map(|kind| String::from_utf8_lossy(kind).into_owned());
            let Some(kind) = kind else {
                return Err("astral.events.on: gateway event types are strings".into());
            };
            types.insert(kind);
            unsafe { lua::pop(state, 1) };
        }
    }
    let subscription = host::next_request();
    let reference = unsafe { reference_at(state, 2) };
    plugin
        .event_callbacks
        .insert(subscription, crate::lua::host::EventCallback { event, reference, types });
    unsafe { lua::lua_pushinteger(state, subscription as i64) };
    Ok(1)
}

pub unsafe fn unsubscribe_event(state: *mut LuaState) -> Outcome {
    let value = unsafe { check_integer(state, 1) }?;
    if value <= 0 {
        return unsafe { push_flag(state, false) };
    }
    let subscription = value as u64;
    let Some(event) = me(state).event_callbacks.get(&subscription).map(|callback| callback.event) else {
        return unsafe { push_flag(state, false) };
    };
    unsafe { require_permission(state, event_permission(event)) }?;
    if let Some(callback) = me(state).event_callbacks.remove(&subscription) {
        unsafe { lua::release(state, callback.reference) };
    }
    unsafe { push_flag(state, true) }
}

pub unsafe fn dispatch_gateway_event(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, Capability::MessagesLocal) }?;
    let kind = unsafe { check_text(state, 1) }?;
    if kind.is_empty() || kind.len() > 64 {
        return Err("astral.gateway.dispatch: event types are 1 to 64 characters".into());
    }
    if !kind.bytes().all(|byte| byte.is_ascii_uppercase() || byte == b'_') {
        return Err(format!("astral.gateway.dispatch: '{kind}' is not a gateway event type"));
    }
    let Some(data) = (unsafe { to_json(state, 2, 0) }) else {
        return Err("astral.gateway.dispatch: data must be JSON-compatible".into());
    };
    let message = json!({"op": 0, "t": kind, "s": null, "d": data}).to_string();
    if message.len() > 1 << 20 {
        return Err("astral.gateway.dispatch: event exceeds 1 MB".into());
    }
    unsafe { push_flag(state, host::platform_flag("dispatchGateway", &json!([message]))) }
}

static LOCAL_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static DECORATOR_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub fn send_local_message_json(channel: &str, message: &Value) -> Option<String> {
    let now = crate::support::bridge::epoch_ms();
    let snowflake = (((now - 1_420_070_400_000) as u64) << 22) | (LOCAL_SEQUENCE.fetch_add(1, Ordering::Relaxed) & 0xFFF);
    let mut data = json!({
        "id": snowflake.to_string(), "channel_id": channel, "type": 0, "content": "", "flags": 64,
        "timestamp": crate::lua::host::api::gateway_timestamp(now),
        "author": {"id": "1", "username": "astral", "global_name": "astral", "discriminator": "0", "bot": true, "public_flags": 0},
        "tts": false, "pinned": false, "mention_everyone": false, "mentions": [], "mention_roles": [], "attachments": [], "embeds": [], "components": [],
    });
    if let Some(guild) = crate::discord::cache::channel_guild(channel).filter(|guild| snowflake_text(guild)) {
        data["guild_id"] = json!(guild);
    }
    for (key, value) in message.as_object().into_iter().flatten() {
        if key != "channel_id" && key != "id" {
            data[key] = value.clone();
        }
    }
    let text = json!({"op": 0, "t": "MESSAGE_CREATE", "s": null, "d": data}).to_string();
    if text.len() > 1 << 20 || !host::platform_flag("dispatchGateway", &json!([text])) {
        return None;
    }
    data["id"].as_str().map(str::to_owned)
}

pub unsafe fn send_local_message(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, Capability::MessagesLocal) }?;
    let channel = unsafe { check_text(state, 1) }?;
    if !snowflake_text(&channel) {
        return Err(format!("astral.messages.send_local: '{channel}' is not a channel id"));
    }
    let message = if unsafe { lua::lua_isstring(state, 2) } != 0 {
        json!({"content": String::from_utf8_lossy(unsafe { lua::to_bytes(state, 2) }.unwrap_or_default())})
    } else {
        let table = if unsafe { lua::is_table(state, 2) } {
            unsafe { to_json(state, 2, 0) }.filter(Value::is_object)
        } else {
            None
        };
        match table {
            Some(table) => table,
            None => return Err("astral.messages.send_local: give text or a message table".into()),
        }
    };
    let Some(id) = send_local_message_json(&channel, &message) else {
        return Ok(0);
    };
    unsafe { lua::push_str(state, &id) };
    Ok(1)
}

unsafe fn register_function(
    state: *mut LuaState,
    permission: Option<Capability>,
    limit: usize,
    message: &str,
    list: impl FnOnce(&mut Plugin) -> &mut Vec<c_int>,
) -> Outcome {
    if let Some(permission) = permission {
        unsafe { require_permission(state, permission) }?;
    }
    unsafe { check_type(state, 1, lua::TFUNCTION) }?;
    let list = list(me(state));
    if list.len() >= limit {
        return Err(message.into());
    }
    let reference = unsafe { reference_at(state, 1) };
    list.push(reference);
    Ok(0)
}

pub unsafe fn register_outgoing_transform(state: *mut LuaState) -> Outcome {
    unsafe {
        register_function(
            state,
            Some(Capability::DiscordMessagesSend),
            16,
            "astral.messages.transform: at most 16 transforms per plugin",
            |plugin| &mut plugin.outgoing_message_transforms,
        )
    }
}

pub unsafe fn register_media_save_handler(state: *mut LuaState) -> Outcome {
    unsafe {
        register_function(
            state,
            Some(Capability::MediaSave),
            4,
            "astral.media.on_save: at most 4 handlers per plugin",
            |plugin| &mut plugin.media_save_handlers,
        )
    }
}

pub unsafe fn register_composer_decorator(state: *mut LuaState) -> Outcome {
    unsafe {
        register_function(
            state,
            Some(Capability::UiComposer),
            1,
            "astral.ui.composer: at most one formatter per plugin",
            |plugin| &mut plugin.composer_decorators,
        )
    }
}

pub unsafe fn register_message_background(state: *mut LuaState) -> Outcome {
    unsafe {
        register_function(
            state,
            Some(Capability::UiMessageBackground),
            1,
            "astral.ui.message_background: at most one decorator per plugin",
            |plugin| &mut plugin.message_background_decorators,
        )
    }
}

pub unsafe fn app_background(state: *mut LuaState) -> Outcome {
    let mut colors = [0u32; crate::wire::abi::PATCH_GRADIENT_STOPS];
    let mut angle = 0.0;
    let count = unsafe { crate::wire::backdrop::astral_wire_app_gradient(colors.as_mut_ptr(), colors.len(), &mut angle) };
    if count == 0 {
        unsafe { lua::lua_pushnil(state) };
        return Ok(1);
    }
    unsafe { crate::lua::json::push_json(state, &json!({ "angle": angle, "colors": colors[..count] }), 0) };
    Ok(1)
}

pub unsafe fn style_text(state: *mut LuaState) -> Outcome {
    unsafe { register_function(state, None, usize::MAX, "", |plugin| &mut plugin.text_stylers) }
}

fn picked_slot(uri: &str) -> Option<String> {
    let slot = uri.strip_prefix("picked://")?;
    let valid = (2..=12).contains(&slot.len()) && slot.starts_with('p') && slot[1..].bytes().all(|byte| byte.is_ascii_digit());
    valid.then(|| slot.to_owned())
}

pub unsafe fn picked_media(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, Capability::MediaPicker) }?;
    let Some(slot) = picked_slot(&unsafe { check_text(state, 1) }?) else {
        return Ok(0);
    };
    let Some(media) = host::platform_value("pickedMedia", &json!([slot])).filter(Value::is_object) else {
        return Ok(0);
    };
    unsafe {
        lua::new_table(state);
        if let Some(size) = media.get("size").and_then(Value::as_i64).filter(|size| *size >= 0) {
            lua::lua_pushinteger(state, size);
            lua::set_field(state, -2, "size");
        }
        lua::lua_pushnumber(state, media.get("duration").and_then(Value::as_f64).unwrap_or(0.0));
        lua::set_field(state, -2, "duration");
        lua::push_str(state, media.get("type").and_then(Value::as_str).unwrap_or_default());
        lua::set_field(state, -2, "type");
    }
    Ok(1)
}

pub unsafe fn picked_video_url(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, Capability::MediaPicker) }?;
    let Some(slot) = picked_slot(&unsafe { check_text(state, 1) }?) else {
        return Ok(0);
    };
    let Some(Value::String(url)) = host::platform_value("pickedVideoURL", &json!([slot])) else {
        return Ok(0);
    };
    unsafe { lua::push_str(state, &url) };
    Ok(1)
}

fn offerable_name(name: &str) -> bool {
    if !(5..=64).contains(&name.len())
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return false;
    }
    if name.starts_with('.') || name.contains("..") {
        return false;
    }
    [".png", ".gif", ".webp", ".jpg", ".jpeg"]
        .iter()
        .any(|extension| name.len() > extension.len() && name.ends_with(extension))
}

pub unsafe fn offer_pick(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, Capability::MediaOffer) }?;
    let bytes = unsafe { check_bytes(state, 1) }?.to_vec();
    let name = unsafe { check_text(state, 2) }?;
    if bytes.is_empty() || bytes.len() > 1024 * 1024 {
        return Err("astral.media.offer_pick: the image is 1 byte to 1 MiB".into());
    }
    if !offerable_name(&name) {
        return Err("astral.media.offer_pick: name is a png, gif, webp or jpg file name".into());
    }
    let encoded = crate::lua::host::install::encode_base64(&bytes);
    unsafe { push_flag(state, host::platform_flag("offerPick", &json!([encoded, name]))) }
}

unsafe fn prefixed_registration(state: *mut LuaState, permission: Capability, function: &str) -> Result<(String, String), String> {
    unsafe { require_permission(state, permission) }?;
    let prefix = unsafe { check_text(state, 1) }?;
    let kind = unsafe { opt_text(state, 2, "") }?;
    unsafe { check_type(state, 3, lua::TFUNCTION) }?;
    if prefix.len() < 3 || prefix.len() >= 96 {
        return Err(format!("{function}: id prefix must be 3 to 95 characters"));
    }
    Ok((prefix, kind))
}

pub unsafe fn register_node_insert(state: *mut LuaState) -> Outcome {
    let (prefix, kind) = unsafe { prefixed_registration(state, Capability::UiInsert, "astral.ui.insert") }?;
    let plugin = me(state);
    if plugin.node_inserts.len() >= 16 {
        return Err("astral.ui.insert: at most 16 inserts per plugin".into());
    }
    let build = unsafe { reference_at(state, 3) };
    let press = if unsafe { lua::is_function(state, 4) } {
        unsafe { reference_at(state, 4) }
    } else {
        NOREF
    };
    let identifier = host::NEXT_INSERT.fetch_add(1, Ordering::Relaxed);
    plugin.node_inserts.push(crate::lua::host::NodeInsert {
        identifier,
        prefix,
        kind,
        build,
        press,
    });
    host::sync_insert_prefixes();
    Ok(0)
}

pub unsafe fn register_node_patch(state: *mut LuaState) -> Outcome {
    let (prefix, kind) = unsafe { prefixed_registration(state, Capability::UiPatch, "astral.ui.patch") }?;
    let plugin = me(state);
    if plugin.node_patches.len() >= 16 {
        return Err("astral.ui.patch: at most 16 patches per plugin".into());
    }
    let reference = unsafe { reference_at(state, 3) };
    plugin.node_patches.push(crate::lua::host::NodePatch { prefix, kind, reference });
    host::sync_patch_prefixes();
    Ok(0)
}

pub unsafe fn decorate(state: *mut LuaState) -> Outcome {
    let name = unsafe { check_text(state, 1) }?;
    unsafe { check_type(state, 2, lua::TFUNCTION) }?;
    let Some(target @ Target::MessageAuthor) = Target::parse(&name) else {
        return Err(format!("astral.ui.decorate: unsupported target '{name}'"));
    };
    let press = if unsafe { lua::is_none_or_nil(state, 3) } {
        None
    } else {
        unsafe { check_type(state, 3, lua::TFUNCTION) }?;
        Some(unsafe { reference_at(state, 3) })
    };
    let reference = unsafe { reference_at(state, 2) };
    let registration = DECORATOR_SEQUENCE.fetch_add(1, Ordering::Relaxed) + 1;
    me(state).decorators.push(crate::lua::host::Decorator {
        target,
        reference,
        registration,
        press,
    });
    crate::lua::host::nodes::forget_tags();
    Ok(0)
}

pub unsafe fn relabel(state: *mut LuaState) -> Outcome {
    let name = unsafe { check_text(state, 1) }?;
    unsafe { check_type(state, 2, lua::TFUNCTION) }?;
    let Some(target) = Target::parse(&name) else {
        return Err(format!("astral.ui.relabel: unsupported target '{name}'"));
    };
    let reference = unsafe { reference_at(state, 2) };
    me(state).relabelers.push((target, reference));
    Ok(0)
}

pub unsafe fn set_message_text(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, Capability::MessagesDisplay) }?;
    let message = unsafe { message_argument(state, 2) }?;
    let texts = &mut host::host().displayed_texts;
    if unsafe { lua::is_none_or_nil(state, 3) } {
        texts.remove(&message);
    } else {
        let text = unsafe { check_bytes(state, 3) }?;
        if text.is_empty() || text.len() > host::MAX_DISPLAYED_TEXT {
            return Err(format!(
                "astral.messages.set_text: text must be 1 to {} bytes",
                host::MAX_DISPLAYED_TEXT
            ));
        }
        let text = String::from_utf8_lossy(text).into_owned();
        let note = unsafe { crate::lua::host::api::table_text(state, 4, c"note") }
            .map(|note| clip(&note, 48))
            .unwrap_or_default();
        if texts.len() >= host::MAX_DISPLAYED_TEXTS
            && !texts.contains_key(&message)
            && let Some(first) = texts.keys().next().copied()
        {
            texts.remove(&first);
        }
        texts.insert(message, (text, note));
    }
    host::refresh_decorations();
    Ok(0)
}

pub unsafe fn get_message_text(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, Capability::MessagesDisplay) }?;
    let message = unsafe { message_argument(state, 1) }?;
    match host::host().displayed_texts.get(&message) {
        Some((text, _)) => unsafe { lua::push_str(state, text) },
        None => unsafe { lua::lua_pushnil(state) },
    }
    Ok(1)
}

pub unsafe fn message_action(state: *mut LuaState) -> Outcome {
    unsafe { register_action(state, "astral.ui.message_action", |plugin| &mut plugin.message_actions) }
}

pub unsafe fn menu_action(state: *mut LuaState) -> Outcome {
    unsafe { register_action(state, "astral.ui.menu_action", |plugin| &mut plugin.menu_actions) }
}

unsafe fn register_action(
    state: *mut LuaState,
    name: &str,
    actions: impl Fn(&mut Plugin) -> &mut Vec<crate::lua::host::MessageAction>,
) -> Outcome {
    let plugin = me(state);
    unsafe { require_permission(state, Capability::UiMessageActions) }?;
    let label = unsafe { check_bytes(state, 1) }?.to_vec();
    unsafe { check_type(state, 2, lua::TFUNCTION) }?;
    if label.is_empty() || label.len() > MESSAGE_ACTION_LABEL_LIMIT {
        return Err(format!("{name}: label must be 1 to 63 bytes"));
    }
    if actions(plugin).len() >= 4 {
        return Err(format!("{name}: at most 4 actions per plugin"));
    }
    let mut icon = "res://ic_mana_puzzle_piece".to_owned();
    let mut when = NOREF;
    let mut danger = false;
    if unsafe { lua::is_table(state, 3) } {
        if let Some(chosen) = unsafe { crate::lua::host::api::table_text(state, 3, c"icon") }
            .filter(|chosen| chosen.starts_with("res://") && chosen.len() < 120)
        {
            icon = chosen;
        }
        danger = unsafe { crate::lua::host::api::table_flag(state, 3, c"danger") };
        unsafe { lua::get_field(state, 3, "when") };
        if unsafe { lua::is_function(state, -1) } {
            when = unsafe { lua::reference(state) };
        } else {
            unsafe { lua::pop(state, 1) };
        }
    }
    let mut identifier = host::NEXT_MESSAGE_ACTION.fetch_add(1, Ordering::Relaxed);
    if identifier == 0 {
        identifier = host::NEXT_MESSAGE_ACTION.fetch_add(1, Ordering::Relaxed);
    }
    let reference = unsafe { reference_at(state, 2) };
    actions(plugin).push(crate::lua::host::MessageAction {
        identifier,
        label: String::from_utf8_lossy(&label).into_owned(),
        icon,
        danger,
        reference,
        when,
    });
    unsafe { lua::lua_pushinteger(state, identifier as i64) };
    Ok(1)
}

fn discord_api_url(url: &str) -> bool {
    let authority = authority(url);
    let host = authority.rsplit_once(':').map_or(authority, |(host, _)| host).to_ascii_lowercase();
    let path = url.get(8 + authority.len()..).unwrap_or_default();
    crate::network::policy::is_discord_host(&host) && (path.starts_with("/api/") || path == "/api")
}

fn valid_request_url(url: &str) -> bool {
    let authority = authority(url);
    url.len() <= 4096
        && url.starts_with("https://")
        && !authority.is_empty()
        && !authority.contains('@')
        && !url.contains('\\')
        && !crate::lua::host::http::contains_control(url)
}

fn network_options(mut options: Value) -> Result<Value, &'static str> {
    if !options.is_object() {
        return Err("astral.net.request: options must be an object");
    }
    let Some(url) = options.get("url").and_then(Value::as_str) else {
        return Err("astral.net.request: url is required");
    };
    if !valid_request_url(url) {
        return Err("astral.net.request: url must be an https URL without credentials");
    }
    if discord_api_url(url) {
        return Err("astral.net.request: Discord's API goes through astral.discord.request");
    }
    let method = match options.get("method") {
        None => "GET".to_owned(),
        Some(Value::String(method)) => method.to_ascii_uppercase(),
        Some(_) => return Err("astral.net.request: method must be a string"),
    };
    if !["DELETE", "GET", "HEAD", "PATCH", "POST", "PUT"].contains(&method.as_str()) {
        return Err("astral.net.request: unsupported method");
    }
    options["method"] = json!(method);
    match options.get("headers") {
        None => options["headers"] = json!({}),
        Some(Value::Object(headers)) if headers.len() <= 32 => {
            for (name, value) in headers {
                let Some(value) = value.as_str().filter(|_| host::safe_header_name(name)) else {
                    return Err("astral.net.request: invalid header");
                };
                if !host::safe_header_value(value) {
                    return Err("astral.net.request: invalid header value");
                }
            }
        }
        Some(_) => return Err("astral.net.request: headers must be an object with at most 32 entries"),
    }
    if let Some(body) = options.get("body") {
        if !body.as_str().is_some_and(|body| body.len() <= 256 << 10) {
            return Err("astral.net.request: body must be a string no larger than 256 KiB");
        }
        if method == "GET" || method == "HEAD" {
            return Err("astral.net.request: GET and HEAD requests cannot have a body");
        }
    }
    let mut timeout = 15000;
    if let Some(value) = options.get("timeout_ms") {
        let Some(requested) = value
            .as_i64()
            .or_else(|| value.as_u64().map(|value| value.min(i64::MAX as u64) as i64))
        else {
            return Err("astral.net.request: timeout_ms must be an integer");
        };
        if !(1000..=30000).contains(&requested) {
            return Err("astral.net.request: timeout_ms must be between 1000 and 30000");
        }
        timeout = requested;
    }
    options["timeout_ms"] = json!(timeout);
    Ok(options)
}

pub unsafe fn network_request(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, Capability::Network) }?;
    if !host::platform_has("networkRequest") {
        return Err("astral.net is unavailable on this client".into());
    }
    unsafe { check_type(state, 1, lua::TTABLE) }?;
    unsafe { check_type(state, 2, lua::TFUNCTION) }?;
    let options = network_options(unsafe { to_json(state, 1, 0) }.unwrap_or(Value::Null)).map_err(str::to_owned)?;
    let request = start_request(|plugin| &mut plugin.network_requests, me(state), state, 2);
    if matches!(
        host::platform("networkRequest", &json!([request, options])),
        Platform::Threw | Platform::Missing
    ) {
        abandon_request(|plugin| &mut plugin.network_requests, me(state), state, request);
        return Err("astral.net.request: could not start the request".into());
    }
    unsafe { lua::lua_pushinteger(state, request as i64) };
    Ok(1)
}

fn discord_path(path: &str) -> bool {
    path.len() <= 2048
        && path.starts_with('/')
        && !path.starts_with("//")
        && !path.contains("..")
        && !path.contains('\\')
        && !path.contains('#')
        && !crate::lua::host::http::contains_control(path)
}

fn discord_request_options(mut options: Value) -> Result<Value, &'static str> {
    if !options.is_object() {
        return Err("astral.discord.request: options must be a table");
    }
    if !options.get("path").and_then(Value::as_str).is_some_and(discord_path) {
        return Err("astral.discord.request: path must start with / and stay inside Discord's API");
    }
    let method = match options.get("method") {
        None => "GET".to_owned(),
        Some(Value::String(method)) => method.to_ascii_uppercase(),
        Some(_) => return Err("astral.discord.request: method must be a string"),
    };
    if !["DELETE", "GET", "PATCH", "POST", "PUT"].contains(&method.as_str()) {
        return Err("astral.discord.request: unsupported method");
    }
    options["method"] = json!(method);
    match options.get("body") {
        None | Some(Value::Null) => {}
        Some(_) if method == "GET" => return Err("astral.discord.request: GET requests cannot have a body"),
        Some(Value::String(body)) if body.len() <= DISCORD_BODY_LIMIT => {}
        Some(Value::String(_)) => return Err("astral.discord.request: body must be no larger than 256 KiB"),
        Some(body) => {
            let encoded = body.to_string();
            if encoded.len() > DISCORD_BODY_LIMIT {
                return Err("astral.discord.request: body must be no larger than 256 KiB");
            }
            options["body"] = json!(encoded);
        }
    }
    Ok(options)
}

pub unsafe fn discord_request(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, Capability::DiscordApi) }?;
    if !host::platform_has("discordRequest") {
        return Err("astral.discord.request is unavailable on this client".into());
    }
    unsafe { check_type(state, 1, lua::TTABLE) }?;
    unsafe { check_type(state, 2, lua::TFUNCTION) }?;
    let options = discord_request_options(unsafe { to_json(state, 1, 0) }.unwrap_or(Value::Null)).map_err(str::to_owned)?;
    let request = start_request(|plugin| &mut plugin.network_requests, me(state), state, 2);
    if matches!(
        host::platform("discordRequest", &json!([request, options])),
        Platform::Threw | Platform::Missing
    ) {
        abandon_request(|plugin| &mut plugin.network_requests, me(state), state, request);
        return Err("astral.discord.request: could not start the request".into());
    }
    unsafe { lua::lua_pushinteger(state, request as i64) };
    Ok(1)
}

pub unsafe fn cancel_network_request(state: *mut LuaState) -> Outcome {
    unsafe {
        cancel_request(
            state,
            Capability::Network,
            |plugin| &mut plugin.network_requests,
            "cancelNetworkRequest",
        )
    }
}

const SHARED_PREFIX: &str = "@";

fn shared_owner(identifier: &str, module: &str) -> Result<&'static Plugin, String> {
    let Some(owner) = super::find(identifier).map(|owner| unsafe { &*owner }) else {
        return Err(format!("shared module '{module}' needs {identifier}, which is not installed"));
    };
    if !super::runnable(owner) {
        return Err(format!("{identifier} is unsigned; turn on developer mode to load its modules"));
    }
    if !owner.package.shared_modules.iter().any(|shared| shared == module) {
        return Err(format!("{identifier} does not share a module named '{module}'"));
    }
    Ok(owner)
}

pub unsafe fn require(state: *mut LuaState) -> Outcome {
    let plugin = me(state);
    let name = unsafe { check_text(state, 1) }?;
    unsafe {
        lua::get_field(state, lua::REGISTRYINDEX, "astral.loaded");
        lua::get_field(state, -1, &name);
    }
    if unsafe { lua::lua_type(state, -1) } != lua::TNIL {
        return Ok(1);
    }
    unsafe { lua::pop(state, 1) };
    let (owner, module) = match name.strip_prefix(SHARED_PREFIX).and_then(|rest| rest.split_once('/')) {
        Some((identifier, module)) => (shared_owner(identifier, module)?, module),
        None => (&*plugin, name.as_str()),
    };
    let Some((source, bytecode)) = owner.package.modules.get(module) else {
        return Err(format!("module '{module}' not found in {}", owner.identifier()));
    };
    let chunk = std::ffi::CString::new(format!("={}/{module}", owner.identifier())).unwrap_or_default();
    let binary = owner.signed && !bytecode.is_empty();
    let code = if binary { bytecode } else { source };
    let mode: &core::ffi::CStr = if binary { c"b" } else { c"t" };
    if unsafe { lua::luaL_loadbufferx(state, code.as_ptr().cast(), code.len(), chunk.as_ptr(), mode.as_ptr()) } != lua::OK {
        let message = String::from_utf8_lossy(unsafe { lua::to_bytes(state, -1) }.unwrap_or_default()).into_owned();
        return Err(message);
    }
    unsafe { lua::push_str(state, &name) };
    if unsafe { lua::pcall(state, 1, 1) } != lua::OK {
        let message = String::from_utf8_lossy(unsafe { lua::to_bytes(state, -1) }.unwrap_or_default()).into_owned();
        return Err(message);
    }
    if unsafe { lua::lua_type(state, -1) } == lua::TNIL {
        unsafe {
            lua::pop(state, 1);
            lua::lua_pushboolean(state, 1);
        }
    }
    unsafe {
        lua::lua_pushvalue(state, -1);
        lua::set_field(state, -3, &name);
    }
    Ok(1)
}
