use crate::lua::enums::{Relationship, ScrollAlign};
use crate::plugins::capability::Capability;
use core::ffi::c_int;
use std::sync::{OnceLock, RwLock};

use serde_json::Value;

use crate::discord::cache as discord;
use crate::lua::api::{self as lua, LuaState};
use crate::lua::json::{push_array_metatable, push_json, to_json};
use crate::plugins::storage::{self as plugin_storage, Storage};
use crate::support::bridge::epoch_ms;

const STORAGE_LIMIT: usize = 512 << 10;
const MAXIMUM_JSON_TEXT: usize = 4 << 20;

static STORAGE_PATH: RwLock<String> = RwLock::new(String::new());
static STORAGE: OnceLock<Storage> = OnceLock::new();

pub type Outcome = Result<c_int, String>;

unsafe fn identifier(state: *mut LuaState) -> String {
    let plugin = crate::lua::host::plugin_of(state);
    if plugin.is_null() {
        String::new()
    } else {
        unsafe { (*plugin).identifier().to_owned() }
    }
}

unsafe fn directory(state: *mut LuaState) -> String {
    let plugin = crate::lua::host::plugin_of(state);
    if plugin.is_null() {
        String::new()
    } else {
        unsafe { (*plugin).package.directory.clone() }
    }
}

use crate::lua::host::Platform;

fn platform_call(method: &str, arguments: &Value) -> Platform {
    crate::lua::host::platform(method, arguments)
}

fn platform_flag(method: &str, arguments: &Value) -> bool {
    crate::lua::host::platform_flag(method, arguments)
}

pub unsafe fn has_permission(state: *mut LuaState, capability: Capability) -> bool {
    let plugin = crate::lua::host::plugin_of(state);
    !plugin.is_null() && unsafe { (*plugin).permissions.contains(&capability) }
}

pub unsafe fn require_permission(state: *mut LuaState, capability: Capability) -> Result<(), String> {
    if unsafe { has_permission(state, capability) } {
        Ok(())
    } else {
        Err(format!("permission '{}' is not declared in manifest.toml", capability.value()))
    }
}

pub fn set_storage_path(path: &str) {
    *STORAGE_PATH.write().unwrap_or_else(|poisoned| poisoned.into_inner()) = path.to_owned();
}

pub fn remove_storage(identifier: &str) {
    storage().remove_all(identifier);
}

pub unsafe fn check_type(state: *mut LuaState, index: c_int, kind: c_int) -> Result<(), String> {
    let actual = unsafe { lua::lua_type(state, index) };
    if actual == kind {
        Ok(())
    } else {
        Err(format!(
            "bad argument #{index} ({} expected, got {})",
            type_name(kind),
            type_name(actual)
        ))
    }
}

pub unsafe fn check_integer(state: *mut LuaState, index: c_int) -> Result<i64, String> {
    if unsafe { lua::lua_isinteger(state, index) } != 0 {
        return Ok(unsafe { lua::to_integer(state, index) });
    }
    let mut converted = 0;
    let value = unsafe { lua::lua_tointegerx(state, index, &mut converted) };
    if converted != 0 {
        return Ok(value);
    }
    let kind = unsafe { lua::lua_type(state, index) };
    if kind == lua::TNUMBER {
        return Err(format!("bad argument #{index} (number has no integer representation)"));
    }
    Err(format!("bad argument #{index} (number expected, got {})", type_name(kind)))
}

pub unsafe fn opt_text(state: *mut LuaState, index: c_int, fallback: &str) -> Result<String, String> {
    if unsafe { lua::is_none_or_nil(state, index) } {
        Ok(fallback.to_owned())
    } else {
        unsafe { check_text(state, index) }
    }
}

pub unsafe fn raise(state: *mut LuaState, message: String) -> c_int {
    unsafe { lua::lua_pushlstring(state, message.as_ptr().cast(), message.len()) };
    drop(message);
    unsafe { lua::lua_error(state) }
}

pub unsafe fn guarded(state: *mut LuaState, body: unsafe fn(*mut LuaState) -> Outcome) -> c_int {
    let outcome = unsafe { body(state) };
    match outcome {
        Ok(results) => results,
        Err(message) => unsafe { raise(state, message) },
    }
}

pub fn type_name(kind: c_int) -> &'static str {
    match kind {
        lua::TNIL => "nil",
        lua::TBOOLEAN => "boolean",
        lua::TNUMBER => "number",
        lua::TSTRING => "string",
        lua::TTABLE => "table",
        -1 => "no value",
        lua::TFUNCTION => "function",
        2 => "userdata",
        _ => "value",
    }
}

pub unsafe fn check_bytes<'a>(state: *mut LuaState, index: c_int) -> Result<&'a [u8], String> {
    let kind = unsafe { lua::lua_type(state, index) };
    if kind != lua::TSTRING && kind != lua::TNUMBER {
        return Err(format!("bad argument #{index} (string expected, got {})", type_name(kind)));
    }
    Ok(unsafe { lua::to_bytes(state, index) }.unwrap_or_default())
}

pub unsafe fn check_text(state: *mut LuaState, index: c_int) -> Result<String, String> {
    Ok(String::from_utf8_lossy(unsafe { check_bytes(state, index) }?).into_owned())
}

pub unsafe fn push_text(state: *mut LuaState, value: &str) {
    unsafe { lua::lua_pushlstring(state, value.as_ptr().cast(), value.len()) };
}

pub unsafe fn table_flag(state: *mut LuaState, index: c_int, field: &core::ffi::CStr) -> bool {
    if unsafe { lua::lua_type(state, index) } != lua::TTABLE {
        return false;
    }
    unsafe {
        lua::lua_getfield(state, index, field.as_ptr());
        let flag = lua::lua_toboolean(state, -1) != 0;
        lua::pop(state, 1);
        flag
    }
}

unsafe fn decode_json(state: *mut LuaState) -> Outcome {
    let text = unsafe { check_bytes(state, 1) }?;
    if text.len() > MAXIMUM_JSON_TEXT {
        return Err("astral.json.decode: text exceeds 4 MB".into());
    }
    let Ok(value) = serde_json::from_slice::<Value>(text) else {
        return Ok(0);
    };
    unsafe { push_json(state, &value, 0) };
    Ok(1)
}

unsafe fn mark_json_array(state: *mut LuaState) -> Outcome {
    let kind = unsafe { lua::lua_type(state, 1) };
    if kind == -1 {
        unsafe { lua::new_table(state) };
    } else if kind != lua::TTABLE {
        return Err(format!("bad argument #1 (table expected, got {})", type_name(kind)));
    }
    unsafe {
        lua::lua_settop(state, 1);
        push_array_metatable(state);
        lua::lua_setmetatable(state, 1);
    }
    Ok(1)
}

fn encode(value: &Value, indent: Option<usize>) -> String {
    let Some(indent) = indent else { return value.to_string() };
    let spaces = vec![b' '; indent];
    let mut output = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(&spaces);
    let mut serializer = serde_json::Serializer::with_formatter(&mut output, formatter);
    if serde::Serialize::serialize(value, &mut serializer).is_err() {
        return value.to_string();
    }
    String::from_utf8(output).unwrap_or_default()
}

unsafe fn encode_json(state: *mut LuaState) -> Outcome {
    let Some(value) = (unsafe { to_json(state, 1, 0) }) else {
        return Err("astral.json.encode: value is not JSON-compatible".into());
    };
    let mut indent = None;
    if unsafe { lua::lua_type(state, 2) } == lua::TTABLE {
        unsafe { lua::lua_getfield(state, 2, c"indent".as_ptr()) };
        if unsafe { lua::lua_isinteger(state, -1) } != 0 {
            indent = Some(unsafe { lua::to_integer(state, -1) }.clamp(0, 8) as usize);
        }
        unsafe { lua::pop(state, 1) };
    }
    let text = encode(&value, indent);
    unsafe { push_text(state, &text) };
    Ok(1)
}

unsafe fn time_now(state: *mut LuaState) -> Outcome {
    unsafe { lua::lua_pushinteger(state, epoch_ms()) };
    Ok(1)
}

pub fn gateway_timestamp(milliseconds: i64) -> String {
    let seconds = milliseconds / 1000;
    let (days, rest) = (seconds / 86400, seconds % 86400);
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}000+00:00",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60,
        milliseconds % 1000
    )
}

fn local_minutes(milliseconds: i64) -> Option<String> {
    let seconds = (milliseconds / 1000) as libc::time_t;
    let mut parts: libc::tm = unsafe { core::mem::zeroed() };
    let mut buffer = [0u8; 32];
    if unsafe { libc::localtime_r(&seconds, &mut parts) }.is_null() {
        return None;
    }
    let written = unsafe { libc::strftime(buffer.as_mut_ptr().cast(), buffer.len(), c"%Y-%m-%d %H:%M".as_ptr(), &parts) };
    (written > 0).then(|| String::from_utf8_lossy(&buffer[..written]).into_owned())
}

unsafe fn time_iso(state: *mut LuaState) -> Outcome {
    let milliseconds = if unsafe { lua::lua_isinteger(state, 1) } != 0 {
        unsafe { lua::to_integer(state, 1) }
    } else {
        epoch_ms()
    };
    if unsafe { table_flag(state, 2, c"local_time") } {
        let Some(text) = local_minutes(milliseconds) else { return Ok(0) };
        unsafe { push_text(state, &text) };
        return Ok(1);
    }
    let mut iso: String = gateway_timestamp(milliseconds).chars().take(16).collect();
    iso.replace_range(10..11, " ");
    iso.push_str(" UTC");
    unsafe { push_text(state, &iso) };
    Ok(1)
}

fn storage() -> &'static Storage {
    STORAGE.get_or_init(|| Storage::open(&STORAGE_PATH.read().unwrap_or_else(|poisoned| poisoned.into_inner()), STORAGE_LIMIT))
}

unsafe fn available_storage(state: *mut LuaState) -> Result<&'static Storage, String> {
    unsafe { require_permission(state, Capability::Storage) }?;
    let storage = storage();
    if storage.available() {
        Ok(storage)
    } else {
        Err("astral.storage is unavailable".into())
    }
}

unsafe fn storage_get(state: *mut LuaState) -> Outcome {
    let storage = unsafe { available_storage(state) }?;
    let key = unsafe { check_text(state, 1) }?;
    match storage.get(&unsafe { identifier(state) }, &key) {
        Some(value) => unsafe { push_json(state, &value, 0) },
        None => unsafe { lua::lua_pushvalue(state, 2) },
    }
    Ok(1)
}

unsafe fn storage_set(state: *mut LuaState) -> Outcome {
    let storage = unsafe { available_storage(state) }?;
    let key = unsafe { check_text(state, 1) }?;
    let Some(value) = (unsafe { to_json(state, 2, 0) }) else {
        return Err(format!("astral.storage.set: '{key}' holds a value that cannot be stored"));
    };
    let plugin = unsafe { identifier(state) };
    match storage.set(&plugin, &key, &value) {
        plugin_storage::SAVED | plugin_storage::REMOVED => Ok(0),
        plugin_storage::OVER_LIMIT => Err(format!("astral.storage: {plugin} would exceed {} KB", STORAGE_LIMIT >> 10)),
        _ => Err(format!("astral.storage.set: could not save '{key}'")),
    }
}

unsafe fn storage_remove(state: *mut LuaState) -> Outcome {
    let storage = unsafe { available_storage(state) }?;
    let key = unsafe { check_text(state, 1) }?;
    storage.remove(&unsafe { identifier(state) }, &key);
    Ok(0)
}

unsafe fn storage_keys(state: *mut LuaState) -> Outcome {
    let storage = unsafe { available_storage(state) }?;
    let keys = storage.keys(&unsafe { identifier(state) });
    unsafe { lua::lua_createtable(state, keys.len() as c_int, 0) };
    for (position, key) in keys.iter().enumerate() {
        unsafe {
            push_text(state, key);
            lua::lua_rawseti(state, -2, position as i64 + 1);
        }
    }
    Ok(1)
}

pub unsafe fn push_optional_json(state: *mut LuaState, value: Option<Value>) {
    match value {
        Some(value) => unsafe { push_json(state, &value, 0) },
        None => unsafe { lua::lua_pushnil(state) },
    }
}

unsafe fn push_optional_text(state: *mut LuaState, value: Option<String>) {
    match value {
        Some(value) => unsafe { push_text(state, &value) },
        None => unsafe { lua::lua_pushnil(state) },
    }
}

pub unsafe fn optional_text(state: *mut LuaState, index: c_int) -> Option<String> {
    let kind = unsafe { lua::lua_type(state, index) };
    (kind == lua::TSTRING || kind == lua::TNUMBER)
        .then(|| String::from_utf8_lossy(unsafe { lua::to_bytes(state, index) }.unwrap_or_default()).into_owned())
}

unsafe fn discord_guild(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, Capability::DiscordGuilds) }?;
    let id = unsafe { check_text(state, 1) }?;
    unsafe { push_optional_json(state, discord::guild(&id)) };
    Ok(1)
}

unsafe fn discord_guilds(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, Capability::DiscordGuilds) }?;
    unsafe { push_json(state, &discord::guilds(), 0) };
    Ok(1)
}

unsafe fn discord_me(state: *mut LuaState) -> Outcome {
    unsafe { push_optional_text(state, discord::current_user()) };
    Ok(1)
}

unsafe fn discord_emoji(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, Capability::DiscordMessages) }?;
    let id = unsafe { check_text(state, 1) }?;
    unsafe { push_optional_json(state, discord::emoji(&id)) };
    Ok(1)
}

unsafe fn discord_member(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, Capability::DiscordMembers) }?;
    let (guild, user) = (unsafe { check_text(state, 1) }?, unsafe { check_text(state, 2) }?);
    unsafe { push_optional_json(state, discord::member(&guild, &user)) };
    Ok(1)
}

unsafe fn discord_channel_guild(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, Capability::DiscordGuilds) }?;
    let channel = unsafe { check_text(state, 1) }?;
    unsafe { push_optional_text(state, discord::channel_guild(&channel)) };
    Ok(1)
}

unsafe fn discord_user(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, Capability::DiscordUsers) }?;
    let user = unsafe { check_text(state, 1) }?;
    unsafe { push_optional_json(state, discord::user(&user)) };
    Ok(1)
}

unsafe fn discord_nickname(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, Capability::DiscordRelationships) }?;
    let user = unsafe { check_text(state, 1) }?;
    unsafe { push_optional_text(state, discord::friend_nickname(&user)) };
    Ok(1)
}

unsafe fn discord_relationship(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, Capability::DiscordRelationships) }?;
    let user = unsafe { check_text(state, 1) }?;
    let relationship = discord::relationship_type(&user).and_then(Relationship::from_discord);
    unsafe { push_optional_text(state, relationship.map(|relationship| relationship.value().to_owned())) };
    Ok(1)
}

unsafe fn discord_ignored(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, Capability::DiscordRelationships) }?;
    let user = unsafe { check_text(state, 1) }?;
    unsafe { lua::lua_pushboolean(state, c_int::from(discord::user_ignored(&user))) };
    Ok(1)
}

unsafe fn discord_muted(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, Capability::DiscordGuilds) }?;
    let channel = unsafe { check_text(state, 1) }?;
    let now = gateway_timestamp(crate::support::bridge::epoch_ms());
    unsafe { lua::lua_pushboolean(state, c_int::from(discord::channel_muted(&channel, &now))) };
    Ok(1)
}

unsafe fn discord_message(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, Capability::DiscordMessages) }?;
    let (channel, message) = (unsafe { check_text(state, 1) }?, unsafe { check_text(state, 2) }?);
    let mut found = discord::message(&channel, &message);
    if !unsafe { has_permission(state, Capability::DiscordMessagesContent) }
        && let Some(object) = found.as_mut().and_then(Value::as_object_mut)
    {
        object.remove("content");
    }
    unsafe { push_optional_json(state, found) };
    Ok(1)
}

unsafe fn discord_raw_message(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, Capability::DiscordMessages) }?;
    unsafe { require_permission(state, Capability::DiscordMessagesContent) }?;
    let (channel, message) = (unsafe { check_text(state, 1) }?, unsafe { check_text(state, 2) }?);
    unsafe { push_optional_json(state, discord::raw_message(&channel, &message)) };
    Ok(1)
}

unsafe fn discord_permissions(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, Capability::DiscordPermissions) }?;
    let (guild, user) = (unsafe { check_text(state, 1) }?, unsafe { check_text(state, 2) }?);
    let channel = unsafe { optional_text(state, 3) };
    match discord::permissions(&guild, &user, channel.as_deref()) {
        Some(granted) => unsafe { lua::lua_pushinteger(state, granted as i64) },
        None => unsafe { lua::lua_pushnil(state) },
    }
    Ok(1)
}

fn snowflake_text(value: &str) -> bool {
    !value.is_empty() && value.len() <= 20 && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn openable_url(url: &str) -> bool {
    url.len() <= 2048 && (url.starts_with("https://") || url.starts_with("discord://"))
}

pub unsafe fn table_text(state: *mut LuaState, index: c_int, field: &core::ffi::CStr) -> Option<String> {
    if unsafe { lua::lua_type(state, index) } != lua::TTABLE {
        return None;
    }
    unsafe { lua::lua_getfield(state, index, field.as_ptr()) };
    let value = unsafe { optional_text(state, -1) };
    unsafe { lua::pop(state, 1) };
    value
}

pub unsafe fn table_number(state: *mut LuaState, index: c_int, field: &core::ffi::CStr) -> Option<f64> {
    if unsafe { lua::lua_type(state, index) } != lua::TTABLE {
        return None;
    }
    unsafe { lua::lua_getfield(state, index, field.as_ptr()) };
    let kind = unsafe { lua::lua_type(state, -1) };
    let mut converted = 0;
    let number = unsafe { lua::lua_tonumberx(state, -1, &mut converted) };
    unsafe { lua::pop(state, 1) };
    (kind == lua::TNUMBER || (kind == lua::TSTRING && converted != 0)).then_some(number)
}

unsafe fn push_flag(state: *mut LuaState, flag: bool) -> Outcome {
    unsafe { lua::lua_pushboolean(state, c_int::from(flag)) };
    Ok(1)
}

fn channel_link(channel: &str, message: &str) -> String {
    let guild = discord::channel_guild(channel)
        .filter(|guild| snowflake_text(guild))
        .unwrap_or_else(|| "@me".into());
    let mut link = format!("https://discord.com/channels/{guild}/{channel}");
    if !message.is_empty() {
        link.push('/');
        link.push_str(message);
    }
    link
}

unsafe fn ui_toast(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, Capability::UiToasts) }?;
    let (title, message) = (unsafe { check_text(state, 1) }?, unsafe { check_text(state, 2) }?);
    if title.len() > 120 || message.len() > 2048 {
        return Err("astral.ui.toast: text exceeds its limit".into());
    }
    match platform_call("showToast", &serde_json::json!([title, message])) {
        Platform::Missing => Err("astral.ui.toast: notifications are unavailable on this build".into()),
        Platform::Threw => Err("astral.ui.toast: could not show the notification".into()),
        Platform::Value(_) => Ok(0),
    }
}

unsafe fn ui_copy(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, Capability::UiClipboard) }?;
    let text = unsafe { check_bytes(state, 1) }?;
    if text.len() > 16384 {
        return Err("astral.ui.copy: text exceeds 16384 bytes".into());
    }
    match platform_call("writeClipboard", &serde_json::json!([String::from_utf8_lossy(text)])) {
        Platform::Missing => Err("astral.ui.copy: clipboard is unavailable on this build".into()),
        reply => unsafe { push_flag(state, matches!(reply, Platform::Value(Value::Bool(true)))) },
    }
}

unsafe fn ui_paste(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, Capability::UiClipboard) }?;
    match platform_call("readClipboard", &serde_json::json!([])) {
        Platform::Missing => Err("astral.ui.paste: clipboard is unavailable on this build".into()),
        Platform::Value(Value::String(text)) => {
            let end = text.len().min(16384);
            unsafe { lua::lua_pushlstring(state, text.as_ptr().cast(), end) };
            Ok(1)
        }
        _ => Ok(0),
    }
}

unsafe fn ui_open(state: *mut LuaState) -> Outcome {
    let url = unsafe { check_text(state, 1) }?;
    if !openable_url(&url) {
        return Err("astral.ui.open: only https:// and discord:// links up to 2048 bytes".into());
    }
    match platform_call("openURL", &serde_json::json!([url])) {
        Platform::Missing => Err("astral.ui.open: opening links is unavailable on this build".into()),
        reply => unsafe { push_flag(state, matches!(reply, Platform::Value(Value::Bool(true)))) },
    }
}

unsafe fn ui_haptic(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, Capability::UiHaptics) }?;
    let strength = unsafe { check_text(state, 1) }?;
    if !matches!(strength.as_str(), "light" | "medium" | "heavy") {
        return Err("astral.ui.haptic: strength must be light, medium or heavy".into());
    }
    match platform_call("triggerHaptic", &serde_json::json!([strength])) {
        Platform::Missing => Err("astral.ui.haptic: haptics are unavailable on this build".into()),
        reply => unsafe { push_flag(state, matches!(reply, Platform::Value(Value::Bool(true)))) },
    }
}

unsafe fn background_keep_alive(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, Capability::Background) }?;
    let on = unsafe { lua::lua_toboolean(state, 1) } != 0;
    unsafe { push_flag(state, platform_flag("keepAlive", &serde_json::json!([on]))) }
}

unsafe fn audio_speak(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, Capability::Audio) }?;
    let text = unsafe { check_text(state, 1) }?;
    if text.is_empty() || text.len() > 4096 {
        return Err("astral.audio.speak: text is 1 to 4096 bytes".into());
    }
    let language = unsafe { table_text(state, 2, c"language") }.unwrap_or_default();
    let rate = unsafe { table_number(state, 2, c"rate") }.unwrap_or(0.5);
    if !rate.is_finite() || !(0.0..=1.0).contains(&rate) {
        return Err("astral.audio.speak: rate is 0 to 1".into());
    }
    if language.len() > 35 {
        return Err("astral.audio.speak: language is a BCP 47 tag".into());
    }
    match platform_call("speak", &serde_json::json!([text, language, rate])) {
        Platform::Missing => Err("astral.audio.speak: speech is unavailable on this build".into()),
        reply => unsafe { push_flag(state, matches!(reply, Platform::Value(Value::Bool(true)))) },
    }
}

fn plugin_sound_path(directory: &str, relative: &str) -> Option<String> {
    let requested = std::path::Path::new(relative);
    if relative.is_empty() || requested.is_absolute() || requested.components().any(|part| part.as_os_str() == "..") {
        return None;
    }
    let extension = requested.extension()?.to_str()?.to_ascii_lowercase();
    if !["mp3", "m4a", "wav", "caf", "aac"].contains(&extension.as_str()) {
        return None;
    }
    let full = std::path::Path::new(directory).join(requested);
    full.is_file().then(|| full.to_string_lossy().into_owned())
}

unsafe fn audio_play(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, Capability::Audio) }?;
    let file = unsafe { check_text(state, 1) }?;
    let volume = unsafe { table_number(state, 2, c"volume") }.unwrap_or(1.0);
    if !volume.is_finite() || !(0.0..=1.0).contains(&volume) {
        return Err("astral.audio.play: volume is 0 to 1".into());
    }
    let Some(path) = plugin_sound_path(&unsafe { directory(state) }, &file) else {
        return Err(format!(
            "astral.audio.play: {file} is not an mp3, m4a, wav, caf or aac file in the plugin"
        ));
    };
    match platform_call("playSound", &serde_json::json!([path, volume])) {
        Platform::Missing => Err("astral.audio.play: sound is unavailable on this build".into()),
        reply => unsafe { push_flag(state, matches!(reply, Platform::Value(Value::Bool(true)))) },
    }
}

unsafe fn audio_stop(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, Capability::Audio) }?;
    platform_call("stopAudio", &serde_json::json!([]));
    Ok(0)
}

static NOTIFICATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

unsafe fn ui_notify(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, Capability::UiNotifications) }?;
    let (title, body) = (unsafe { check_text(state, 1) }?, unsafe { check_text(state, 2) }?);
    if title.len() > 120 || body.len() > 2048 {
        return Err("astral.ui.notify: text exceeds its limit".into());
    }
    let delay = unsafe { table_number(state, 3, c"delay") }.unwrap_or(0.0);
    let channel = unsafe { table_text(state, 3, c"channel_id") }.unwrap_or_default();
    let message = unsafe { table_text(state, 3, c"message_id") }.unwrap_or_default();
    if (!channel.is_empty() && !snowflake_text(&channel)) || (!message.is_empty() && (channel.is_empty() || !snowflake_text(&message))) {
        return Err("astral.ui.notify: channel_id and message_id must be Discord ids".into());
    }
    let link = if channel.is_empty() {
        String::new()
    } else {
        channel_link(&channel, &message)
    };
    if !delay.is_finite() || !(0.0..=604_800.0).contains(&delay) {
        return Err("astral.ui.notify: delay is 0 to 604800 seconds".into());
    }
    let identifier = format!(
        "astral.{}.{}",
        unsafe { identifier(state) },
        NOTIFICATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    match platform_call("notify", &serde_json::json!([identifier, title, body, delay, link])) {
        Platform::Missing => Err("astral.ui.notify: notifications are unavailable on this build".into()),
        reply => unsafe { push_flag(state, matches!(reply, Platform::Value(Value::Bool(true)))) },
    }
}

unsafe fn ui_open_channel(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, Capability::UiNavigate) }?;
    let channel = unsafe { check_text(state, 1) }?;
    let message = unsafe { optional_text(state, 2) }.unwrap_or_default();
    if !snowflake_text(&channel) {
        return Err(format!("astral.ui.open_channel: '{channel}' is not a channel id"));
    }
    if !message.is_empty() && !snowflake_text(&message) {
        return Err(format!("astral.ui.open_channel: '{message}' is not a message id"));
    }
    unsafe {
        push_flag(
            state,
            platform_flag("openURL", &serde_json::json!([channel_link(&channel, &message)])),
        )
    }
}

pub unsafe fn message_argument(state: *mut LuaState, index: c_int) -> Result<u64, String> {
    let text = unsafe { check_text(state, index) }?;
    let digits = text.trim_start().trim_start_matches('+');
    match digits.parse::<u64>() {
        Ok(parsed) if parsed != 0 => Ok(parsed),
        _ => Err(format!("astral.messages: '{text}' is not a message id")),
    }
}

unsafe fn ui_scroll_to(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, Capability::UiNavigate) }?;
    let message = unsafe { message_argument(state, 1) }?;
    let mut animated = true;
    if unsafe { lua::lua_type(state, 2) } == lua::TTABLE {
        unsafe { lua::lua_getfield(state, 2, c"animated".as_ptr()) };
        if unsafe { lua::lua_type(state, -1) } != lua::TNIL {
            animated = unsafe { lua::lua_toboolean(state, -1) } != 0;
        }
        unsafe { lua::pop(state, 1) };
    }
    let align = unsafe { table_text(state, 2, c"align") }
        .as_deref()
        .and_then(ScrollAlign::parse)
        .unwrap_or(ScrollAlign::Visible)
        .value();
    unsafe {
        push_flag(
            state,
            platform_flag("scrollToMessage", &serde_json::json!([message, animated, align])),
        )
    }
}

unsafe fn ui_focus_composer(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, Capability::UiNavigate) }?;
    let kind = unsafe { lua::lua_type(state, 1) };
    let focus = kind == lua::TNIL || kind == -1 || unsafe { lua::lua_toboolean(state, 1) } != 0;
    unsafe { push_flag(state, platform_flag("focusComposer", &serde_json::json!([focus]))) }
}

unsafe fn ui_dismiss(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, Capability::UiNavigate) }?;
    unsafe { push_flag(state, platform_flag("dismissSheets", &serde_json::json!([]))) }
}

unsafe fn ui_insert_text(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, Capability::UiComposer) }?;
    let text = unsafe { check_bytes(state, 1) }?;
    let replace = unsafe { table_flag(state, 2, c"replace") };
    if (text.is_empty() && !replace) || text.len() > 4000 {
        return Err("astral.ui.insert_text: text is 1 to 4000 bytes".into());
    }
    let text = String::from_utf8_lossy(text).into_owned();
    unsafe { push_flag(state, platform_flag("insertComposerText", &serde_json::json!([text, replace]))) }
}

unsafe fn ui_composer_text(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, Capability::UiComposer) }?;
    let mut buffer = vec![0u8; 8192];
    let length = unsafe { crate::wire::composer::astral_wire_composer_text(buffer.as_mut_ptr().cast(), buffer.len()) };
    unsafe { lua::lua_pushlstring(state, buffer.as_ptr().cast(), length.min(buffer.len())) };
    Ok(1)
}

unsafe fn ui_refresh(_state: *mut LuaState) -> Outcome {
    platform_call("refreshDecorations", &serde_json::json!([]));
    Ok(0)
}

unsafe fn diagnostics_environment(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, Capability::Diagnostics) }?;
    let mut environment = match platform_call("environment", &serde_json::json!([])) {
        Platform::Value(value @ Value::Object(_)) => value,
        _ => serde_json::json!({}),
    };
    environment["plugin_list"] = crate::lua::host::plugin_list();
    unsafe { push_json(state, &environment, 0) };
    Ok(1)
}

unsafe fn code_highlight(state: *mut LuaState) -> Outcome {
    let code = unsafe { check_bytes(state, 1) }?.to_vec();
    let language = unsafe { optional_text(state, 2) }.unwrap_or_default();
    let theme = unsafe { optional_text(state, 3) }.unwrap_or_else(|| "{}".into());
    let result = crate::support::bridge::take(unsafe {
        crate::render::highlight::astral_code_highlight(
            code.as_ptr().cast(),
            code.len(),
            language.as_ptr().cast(),
            language.len(),
            theme.as_ptr().cast(),
            theme.len(),
        )
    });
    let value: Value = serde_json::from_slice(&result).unwrap_or(Value::Null);
    unsafe { push_json(state, &value, 0) };
    Ok(1)
}

macro_rules! lua_function {
    ($name:ident, $body:ident) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(state: *mut LuaState) -> c_int {
            unsafe { guarded(state, $body) }
        }
    };
}

lua_function!(astral_lua_json_decode, decode_json);
lua_function!(astral_lua_json_encode, encode_json);
lua_function!(astral_lua_json_array, mark_json_array);
lua_function!(astral_lua_time_now_ms, time_now);
lua_function!(astral_lua_time_iso, time_iso);
lua_function!(astral_lua_storage_get, storage_get);
lua_function!(astral_lua_storage_set, storage_set);
lua_function!(astral_lua_storage_remove, storage_remove);
lua_function!(astral_lua_storage_keys, storage_keys);
lua_function!(astral_lua_discord_guild, discord_guild);
lua_function!(astral_lua_discord_guilds, discord_guilds);
lua_function!(astral_lua_discord_me, discord_me);
lua_function!(astral_lua_discord_emoji, discord_emoji);
lua_function!(astral_lua_discord_member, discord_member);
lua_function!(astral_lua_discord_channel_guild, discord_channel_guild);
lua_function!(astral_lua_discord_user, discord_user);
lua_function!(astral_lua_discord_nickname, discord_nickname);
lua_function!(astral_lua_discord_relationship, discord_relationship);
lua_function!(astral_lua_discord_ignored, discord_ignored);
lua_function!(astral_lua_discord_muted, discord_muted);
lua_function!(astral_lua_discord_message, discord_message);
lua_function!(astral_lua_discord_raw_message, discord_raw_message);
lua_function!(astral_lua_discord_permissions, discord_permissions);
lua_function!(astral_lua_ui_toast, ui_toast);
lua_function!(astral_lua_ui_copy, ui_copy);
lua_function!(astral_lua_ui_paste, ui_paste);
lua_function!(astral_lua_ui_open, ui_open);
lua_function!(astral_lua_ui_haptic, ui_haptic);
lua_function!(astral_lua_background_keep_alive, background_keep_alive);
lua_function!(astral_lua_audio_speak, audio_speak);
lua_function!(astral_lua_audio_play, audio_play);
lua_function!(astral_lua_audio_stop, audio_stop);
lua_function!(astral_lua_ui_notify, ui_notify);
lua_function!(astral_lua_ui_open_channel, ui_open_channel);
lua_function!(astral_lua_ui_scroll_to, ui_scroll_to);
lua_function!(astral_lua_ui_focus_composer, ui_focus_composer);
lua_function!(astral_lua_ui_dismiss, ui_dismiss);
lua_function!(astral_lua_ui_insert_text, ui_insert_text);
lua_function!(astral_lua_ui_composer_text, ui_composer_text);
lua_function!(astral_lua_ui_refresh, ui_refresh);
lua_function!(astral_lua_diagnostics_environment, diagnostics_environment);
lua_function!(astral_lua_code_highlight, code_highlight);
