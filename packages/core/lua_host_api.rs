use core::ffi::{c_char, c_int, c_void};
use std::sync::{OnceLock, RwLock};

use serde_json::Value;

use crate::bridge::{AstralRustString, epoch_ms, text};
use crate::discord_cache as discord;
use crate::lua_api::{self as lua, LuaState, REGISTRYINDEX};
use crate::lua_json::{push_array_metatable, push_json, to_json};
use crate::plugin_storage::{self, Storage};

const STORAGE_LIMIT: usize = 512 << 10;
const MAXIMUM_JSON_TEXT: usize = 4 << 20;

pub type HasPermission = unsafe extern "C" fn(plugin: *mut c_void, name: *const c_char, length: usize) -> c_int;
pub type Identifier = unsafe extern "C" fn(plugin: *mut c_void) -> AstralRustString;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AstralLuaPluginCalls {
    pub has_permission: Option<HasPermission>,
    pub identifier: Option<Identifier>,
}

static CALLS: RwLock<AstralLuaPluginCalls> = RwLock::new(AstralLuaPluginCalls {
    has_permission: None,
    identifier: None,
});
static STORAGE_PATH: RwLock<String> = RwLock::new(String::new());
static STORAGE: OnceLock<Storage> = OnceLock::new();

type Outcome = Result<c_int, String>;

fn calls() -> AstralLuaPluginCalls {
    *CALLS.read().unwrap_or_else(|poisoned| poisoned.into_inner())
}

unsafe fn plugin(state: *mut LuaState) -> *mut c_void {
    unsafe { lua::lua_touserdata(state, REGISTRYINDEX - 1) }
}

unsafe fn identifier(state: *mut LuaState) -> String {
    let Some(identifier) = calls().identifier else {
        return String::new();
    };
    String::from_utf8_lossy(&crate::bridge::take(unsafe { identifier(plugin(state)) })).into_owned()
}

pub unsafe fn has_permission(state: *mut LuaState, name: &str) -> bool {
    calls()
        .has_permission
        .is_some_and(|check| unsafe { check(plugin(state), name.as_ptr().cast(), name.len()) } != 0)
}

unsafe fn require_permission(state: *mut LuaState, name: &str) -> Result<(), String> {
    if unsafe { has_permission(state, name) } {
        Ok(())
    } else {
        Err(format!("permission '{name}' is not declared in manifest.toml"))
    }
}

unsafe fn raise(state: *mut LuaState, message: String) -> c_int {
    unsafe { lua::lua_pushlstring(state, message.as_ptr().cast(), message.len()) };
    drop(message);
    unsafe { lua::lua_error(state) }
}

unsafe fn guarded(state: *mut LuaState, body: unsafe fn(*mut LuaState) -> Outcome) -> c_int {
    let outcome = unsafe { body(state) };
    match outcome {
        Ok(results) => results,
        Err(message) => unsafe { raise(state, message) },
    }
}

fn type_name(kind: c_int) -> &'static str {
    match kind {
        lua::TNIL => "nil",
        lua::TBOOLEAN => "boolean",
        lua::TNUMBER => "number",
        lua::TSTRING => "string",
        lua::TTABLE => "table",
        -1 => "no value",
        _ => "value",
    }
}

unsafe fn check_bytes<'a>(state: *mut LuaState, index: c_int) -> Result<&'a [u8], String> {
    let kind = unsafe { lua::lua_type(state, index) };
    if kind != lua::TSTRING && kind != lua::TNUMBER {
        return Err(format!("bad argument #{index} (string expected, got {})", type_name(kind)));
    }
    Ok(unsafe { lua::to_bytes(state, index) }.unwrap_or_default())
}

unsafe fn check_text(state: *mut LuaState, index: c_int) -> Result<String, String> {
    Ok(String::from_utf8_lossy(unsafe { check_bytes(state, index) }?).into_owned())
}

unsafe fn push_text(state: *mut LuaState, value: &str) {
    unsafe { lua::lua_pushlstring(state, value.as_ptr().cast(), value.len()) };
}

unsafe fn table_flag(state: *mut LuaState, index: c_int, field: &core::ffi::CStr) -> bool {
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
    unsafe { require_permission(state, "storage") }?;
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

unsafe fn push_optional_json(state: *mut LuaState, value: Option<Value>) {
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

unsafe fn optional_text(state: *mut LuaState, index: c_int) -> Option<String> {
    let kind = unsafe { lua::lua_type(state, index) };
    (kind == lua::TSTRING || kind == lua::TNUMBER)
        .then(|| String::from_utf8_lossy(unsafe { lua::to_bytes(state, index) }.unwrap_or_default()).into_owned())
}

unsafe fn discord_guild(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, "discord.guilds") }?;
    let id = unsafe { check_text(state, 1) }?;
    unsafe { push_optional_json(state, discord::guild(&id)) };
    Ok(1)
}

unsafe fn discord_guilds(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, "discord.guilds") }?;
    unsafe { push_json(state, &discord::guilds(), 0) };
    Ok(1)
}

unsafe fn discord_me(state: *mut LuaState) -> Outcome {
    unsafe { push_optional_text(state, discord::current_user()) };
    Ok(1)
}

unsafe fn discord_emoji(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, "discord.messages") }?;
    let id = unsafe { check_text(state, 1) }?;
    unsafe { push_optional_json(state, discord::emoji(&id)) };
    Ok(1)
}

unsafe fn discord_member(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, "discord.members") }?;
    let (guild, user) = (unsafe { check_text(state, 1) }?, unsafe { check_text(state, 2) }?);
    unsafe { push_optional_json(state, discord::member(&guild, &user)) };
    Ok(1)
}

unsafe fn discord_channel_guild(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, "discord.guilds") }?;
    let channel = unsafe { check_text(state, 1) }?;
    unsafe { push_optional_text(state, discord::channel_guild(&channel)) };
    Ok(1)
}

unsafe fn discord_user(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, "discord.users") }?;
    let user = unsafe { check_text(state, 1) }?;
    unsafe { push_optional_json(state, discord::user(&user)) };
    Ok(1)
}

unsafe fn discord_nickname(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, "discord.relationships") }?;
    let user = unsafe { check_text(state, 1) }?;
    unsafe { push_optional_text(state, discord::friend_nickname(&user)) };
    Ok(1)
}

unsafe fn discord_relationship(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, "discord.relationships") }?;
    let user = unsafe { check_text(state, 1) }?;
    let name = match discord::relationship_type(&user) {
        Some(1) => Some("friend"),
        Some(2) => Some("blocked"),
        Some(3) => Some("incoming"),
        Some(4) => Some("outgoing"),
        _ => None,
    };
    unsafe { push_optional_text(state, name.map(str::to_owned)) };
    Ok(1)
}

unsafe fn discord_message(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, "discord.messages") }?;
    let (channel, message) = (unsafe { check_text(state, 1) }?, unsafe { check_text(state, 2) }?);
    let mut found = discord::message(&channel, &message);
    if !unsafe { has_permission(state, "discord.messages.content") }
        && let Some(object) = found.as_mut().and_then(Value::as_object_mut)
    {
        object.remove("content");
    }
    unsafe { push_optional_json(state, found) };
    Ok(1)
}

unsafe fn discord_raw_message(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, "discord.messages") }?;
    unsafe { require_permission(state, "discord.messages.content") }?;
    let (channel, message) = (unsafe { check_text(state, 1) }?, unsafe { check_text(state, 2) }?);
    unsafe { push_optional_json(state, discord::raw_message(&channel, &message)) };
    Ok(1)
}

unsafe fn discord_permissions(state: *mut LuaState) -> Outcome {
    unsafe { require_permission(state, "discord.permissions") }?;
    let (guild, user) = (unsafe { check_text(state, 1) }?, unsafe { check_text(state, 2) }?);
    let channel = unsafe { optional_text(state, 3) };
    match discord::permissions(&guild, &user, channel.as_deref()) {
        Some(granted) => unsafe { lua::lua_pushinteger(state, granted as i64) },
        None => unsafe { lua::lua_pushnil(state) },
    }
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
lua_function!(astral_lua_discord_message, discord_message);
lua_function!(astral_lua_discord_raw_message, discord_raw_message);
lua_function!(astral_lua_discord_permissions, discord_permissions);

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_lua_api_configure(calls: AstralLuaPluginCalls, storage_path: *const c_char, length: usize) {
    *CALLS.write().unwrap_or_else(|poisoned| poisoned.into_inner()) = calls;
    *STORAGE_PATH.write().unwrap_or_else(|poisoned| poisoned.into_inner()) = text(storage_path, length);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_lua_storage_remove_all(identifier: *const c_char, length: usize) {
    storage().remove_all(&text(identifier, length));
}
