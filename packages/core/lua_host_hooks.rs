use core::ffi::{CStr, c_char, c_int};

use serde_json::Value;

use crate::lua_api::{self as lua, LuaState};
use crate::lua_host::{self as host, Plugin};
use crate::wire::abi::{
    AstralLabelContext, AstralMentionContext, AstralMentionDecoration, AstralNodePatch, AstralPatchTarget, LABEL_REPLY, PATCH_BACKGROUND,
    PATCH_BORDER, PATCH_HIDDEN, PATCH_MARGIN, PATCH_OPACITY, PATCH_PADDING, PATCH_RADIUS, PATCH_TEXT_COLOR, PATCH_TEXT_SCALE,
};

pub fn snowflake(value: u64) -> String {
    if value == 0 { String::new() } else { value.to_string() }
}

pub unsafe fn c_text(pointer: *const c_char) -> String {
    if pointer.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(pointer) }.to_string_lossy().into_owned()
    }
}

pub fn write_text(out: &mut [c_char], text: &str) {
    if out.is_empty() {
        return;
    }
    let bytes = text.as_bytes();
    let length = bytes.len().min(out.len() - 1);
    for (slot, byte) in out.iter_mut().zip(&bytes[..length]) {
        *slot = *byte as c_char;
    }
    out[length] = 0;
}

pub unsafe fn write_raw(out: *mut c_char, capacity: usize, text: &str) -> c_int {
    if out.is_null() || capacity == 0 {
        return text.len() as c_int;
    }
    let slice = unsafe { core::slice::from_raw_parts_mut(out, capacity) };
    write_text(slice, text);
    text.len() as c_int
}

static LAST_CHANNEL: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

pub fn resolve_channel(channel_id: u64, message: &str) -> String {
    let channel = snowflake(channel_id);
    if !channel.is_empty() || message.is_empty() {
        return channel;
    }
    let mut last = LAST_CHANNEL.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(found) = crate::discord_cache::message_channel(message) {
        *last = found;
    }
    last.clone()
}

pub fn message_author(channel: &str, message: &str) -> String {
    if channel.is_empty() || message.is_empty() {
        return String::new();
    }
    let user = crate::discord_cache::message(channel, message)
        .and_then(|record| {
            record
                .get("author")
                .and_then(|author| author.get("id"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_default();
    if crate::lua_host_ui::snowflake_text(&user) {
        user
    } else {
        String::new()
    }
}

pub fn guild_of(channel: &str) -> Option<String> {
    if channel.is_empty() {
        None
    } else {
        crate::discord_cache::channel_guild(channel)
    }
}

pub fn opaque(value: i64) -> u32 {
    let color = value as u32;
    if color >> 24 != 0 { color } else { color | 0xFF00_0000 }
}

pub unsafe fn set_text(state: *mut LuaState, key: &str, value: &str) {
    unsafe {
        lua::push_str(state, value);
        lua::set_field(state, -2, key);
    }
}

pub unsafe fn set_optional(state: *mut LuaState, key: &str, value: &str) {
    if !value.is_empty() {
        unsafe { set_text(state, key, value) };
    }
}

pub fn running_plugins() -> Vec<*mut Plugin> {
    host::plugins()
        .into_iter()
        .filter(|plugin| unsafe { (**plugin).running() })
        .collect()
}

pub fn busy<T>(fallback: T) -> T {
    host::redraw_after_busy();
    fallback
}

fn label_target(kind: c_int) -> &'static str {
    match kind {
        2 => "channel.name",
        3 => "member.name",
        4 => "composer.placeholder",
        5 => "profile.name",
        _ => "message.author",
    }
}

fn label_kind_name(kind: c_int) -> &'static str {
    match kind {
        LABEL_REPLY => "reply",
        2 => "channel",
        3 => "member",
        4 => "composer",
        5 => "profile",
        _ => "author",
    }
}

#[unsafe(export_name = "AstralLuaRelabel")]
pub unsafe extern "C" fn astral_lua_relabel(
    context: *const AstralLabelContext,
    text: *const c_char,
    length: usize,
    out: *mut c_char,
    capacity: usize,
) -> c_int {
    let Some(_held) = host::try_lock() else { return busy(-1) };
    let Some(context) = (unsafe { context.as_ref() }) else { return -1 };
    let message = snowflake(context.message_id);
    let channel = resolve_channel(context.channel_id, &message);
    let target = snowflake(context.target_id);
    let guild = guild_of(&channel);
    let wanted = label_target(context.kind);
    let mut current = String::from_utf8_lossy(crate::bridge::bytes(text, length)).into_owned();
    let mut changed = false;
    for plugin in running_plugins() {
        let relabelers: Vec<c_int> = unsafe { &*plugin }
            .relabelers
            .iter()
            .filter(|(target, _)| target == wanted)
            .map(|(_, reference)| *reference)
            .collect();
        for reference in relabelers {
            let state = unsafe { (*plugin).state() };
            unsafe {
                lua::push_reference(state, reference);
                lua::lua_createtable(state, 0, 6);
                set_text(state, "kind", label_kind_name(context.kind));
                set_text(state, "text", &current);
                set_optional(state, "message_id", &message);
                set_optional(state, "channel_id", &channel);
                if context.kind == 3 || context.kind == 5 {
                    set_optional(state, "user_id", &target);
                }
                if let Some(guild) = &guild {
                    set_text(state, "guild_id", guild);
                }
            }
            if host::call(plugin, 1, 1, "relabel") {
                if unsafe { lua::lua_type(state, -1) } == lua::TSTRING
                    && let Some(result) = unsafe { lua::to_bytes(state, -1) }.filter(|result| !result.is_empty() && result.len() < capacity)
                {
                    current = String::from_utf8_lossy(result).into_owned();
                    changed = true;
                }
                unsafe { lua::pop(state, 1) };
            }
            if unsafe { (*plugin).failed } {
                break;
            }
        }
        if unsafe { (*plugin).failed } {
            host::stop_plugin(plugin);
        }
    }
    if !changed {
        return -1;
    }
    unsafe { write_raw(out, capacity, &current) }
}

#[unsafe(export_name = "AstralLuaMention")]
pub unsafe extern "C" fn astral_lua_mention(context: *const AstralMentionContext, decoration: *mut AstralMentionDecoration) -> c_int {
    let Some(_held) = host::try_lock() else { return busy(0) };
    let (Some(context), Some(decoration)) = (unsafe { context.as_ref() }, unsafe { decoration.as_mut() }) else {
        return 0;
    };
    let message = snowflake(context.message_id);
    let channel = resolve_channel(context.channel_id, &message);
    let guild = guild_of(&channel);
    let text = unsafe { c_text(context.text) };
    let mut decorated = 0;
    for plugin in running_plugins() {
        let mentioners = unsafe { &*plugin }.mentioners.clone();
        for reference in mentioners {
            let state = unsafe { (*plugin).state() };
            unsafe {
                lua::push_reference(state, reference);
                lua::lua_createtable(state, 0, 5);
                set_text(state, "text", &text);
                lua::lua_pushinteger(state, i64::from(context.index) + 1);
                lua::set_field(state, -2, "index");
                set_text(state, "message_id", &message);
                set_optional(state, "channel_id", &channel);
                if let Some(guild) = &guild {
                    set_text(state, "guild_id", guild);
                }
            }
            if host::call(plugin, 1, 1, "mention") {
                if unsafe { lua::is_table(state, -1) } {
                    unsafe {
                        lua::get_field(state, -1, "after");
                        if lua::lua_toboolean(state, -1) != 0 {
                            decoration.after = 1;
                        }
                        lua::pop(state, 1);
                        lua::get_field(state, -1, "color");
                        if lua::lua_isinteger(state, -1) != 0 && decoration.color == 0 {
                            decoration.color = opaque(lua::to_integer(state, -1));
                        }
                        lua::pop(state, 1);
                        lua::get_field(state, -1, "image");
                        lua::get_field(state, -2, "round");
                        lua::get_field(state, -3, "hide_at");
                        let image = lua::to_bytes(state, -3)
                            .map(|image| String::from_utf8_lossy(image).into_owned())
                            .unwrap_or_default();
                        if !image.is_empty()
                            && decoration.image[0] == 0
                            && (image.starts_with("https://") || image.starts_with("res://astral_tinted_"))
                        {
                            write_text(&mut decoration.image, &image);
                            decoration.round = if lua::lua_type(state, -2) == lua::TNIL {
                                1
                            } else {
                                lua::lua_toboolean(state, -2)
                            };
                        }
                        if lua::lua_toboolean(state, -1) != 0 {
                            decoration.hide_at = 1;
                        }
                        lua::pop(state, 3);
                    }
                    decorated = 1;
                }
                unsafe { lua::pop(state, 1) };
            }
            if unsafe { (*plugin).failed } {
                break;
            }
        }
        if unsafe { (*plugin).failed } {
            host::stop_plugin(plugin);
        }
    }
    decorated
}

fn replies_to_current_user(record: &Value, current_user: &str) -> bool {
    let author = record
        .get("author")
        .and_then(|author| author.get("id"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let Some(reference) = record.get("reference").filter(|reference| reference.is_object()) else {
        return false;
    };
    if author.is_empty() || author == current_user {
        return false;
    }
    let channel = reference.get("channel_id").and_then(Value::as_str).unwrap_or_default();
    let message = reference.get("message_id").and_then(Value::as_str).unwrap_or_default();
    if channel.is_empty() || message.is_empty() {
        return false;
    }
    crate::discord_cache::message(channel, message)
        .and_then(|target| {
            target
                .get("author")
                .and_then(|author| author.get("id"))
                .and_then(Value::as_str)
                .map(|id| id == current_user)
        })
        .unwrap_or(false)
}

#[unsafe(export_name = "AstralLuaMessageTextColor")]
pub unsafe extern "C" fn astral_lua_message_text_color(message_id: u64, channel_id: u64, color: *mut u32) -> c_int {
    let Some(_held) = host::try_lock() else { return busy(0) };
    let colorizing: Vec<*mut Plugin> = running_plugins()
        .into_iter()
        .filter(|plugin| unsafe { !(**plugin).colorizers.is_empty() })
        .collect();
    if colorizing.is_empty() {
        return 0;
    }
    let wants_reply_context = colorizing
        .iter()
        .any(|plugin| unsafe { (**plugin).permissions.contains("discord.messages") });
    let message = snowflake(message_id);
    let channel = resolve_channel(channel_id, &message);
    if channel.is_empty() {
        return -1;
    }
    let Some(record) = crate::discord_cache::message(&channel, &message) else {
        return -1;
    };
    let current_user = host::host().current_user_id.clone();
    let replies = wants_reply_context && !current_user.is_empty() && replies_to_current_user(&record, &current_user);
    let guild = crate::discord_cache::channel_guild(&channel);
    for plugin in running_plugins() {
        let colorizers = unsafe { &*plugin }.colorizers.clone();
        for reference in colorizers {
            let state = unsafe { (*plugin).state() };
            unsafe {
                lua::push_reference(state, reference);
                lua::lua_createtable(state, 0, 4);
                set_text(state, "message_id", &message);
                set_text(state, "channel_id", &channel);
                if (*plugin).permissions.contains("discord.messages") {
                    lua::lua_pushboolean(state, c_int::from(replies));
                    lua::set_field(state, -2, "reply_to_me");
                }
                if let Some(guild) = &guild {
                    set_text(state, "guild_id", guild);
                }
            }
            if host::call(plugin, 1, 1, "message_color") {
                let found = unsafe { lua::lua_isinteger(state, -1) } != 0;
                if found && let Some(color) = unsafe { color.as_mut() } {
                    *color = unsafe { lua::to_integer(state, -1) } as u32;
                }
                unsafe { lua::pop(state, 1) };
                if found {
                    return 1;
                }
            }
            if unsafe { (*plugin).failed } {
                break;
            }
        }
        if unsafe { (*plugin).failed } {
            host::stop_plugin(plugin);
        }
    }
    0
}

pub unsafe fn patch_color(state: *mut LuaState, field: &str) -> Option<u32> {
    unsafe { lua::get_field(state, -1, field) };
    let value = unsafe { lua::to_integer(state, -1) };
    let valid = unsafe { lua::lua_isinteger(state, -1) } != 0 && (0..=i64::from(u32::MAX)).contains(&value);
    unsafe { lua::pop(state, 1) };
    valid.then_some(value as u32)
}

pub unsafe fn patch_number(state: *mut LuaState, field: &str, minimum: f64, maximum: f64) -> Option<f64> {
    unsafe { lua::get_field(state, -1, field) };
    let value = unsafe { lua::to_number(state, -1) };
    let valid = unsafe { lua::lua_isnumber(state, -1) } != 0 && value.is_finite() && (minimum..=maximum).contains(&value);
    unsafe { lua::pop(state, 1) };
    valid.then_some(value)
}

pub unsafe fn patch_quad(state: *mut LuaState, field: &str) -> Option<[f32; 4]> {
    unsafe { lua::get_field(state, -1, field) };
    let mut quad = [0f32; 4];
    let mut valid = false;
    if unsafe { lua::lua_isnumber(state, -1) } != 0 {
        let value = unsafe { lua::to_number(state, -1) };
        valid = value.is_finite() && (-64.0..=64.0).contains(&value);
        quad = [value as f32; 4];
    } else if unsafe { lua::is_table(state, -1) } && unsafe { lua::luaL_len(state, -1) } == 4 {
        valid = true;
        for (index, slot) in quad.iter_mut().enumerate() {
            unsafe { lua::lua_rawgeti(state, -1, index as i64 + 1) };
            let value = if unsafe { lua::lua_isnumber(state, -1) } != 0 {
                unsafe { lua::to_number(state, -1) }
            } else {
                f64::NAN
            };
            valid = valid && value.is_finite() && (-64.0..=64.0).contains(&value);
            *slot = value as f32;
            unsafe { lua::pop(state, 1) };
        }
    }
    unsafe { lua::pop(state, 1) };
    valid.then_some(quad)
}

pub unsafe fn read_node_patch(state: *mut LuaState, patch: &mut AstralNodePatch) {
    unsafe {
        if let Some(color) = patch_color(state, "background") {
            patch.background = color;
            patch.fields |= PATCH_BACKGROUND;
        }
        if let Some(radius) = patch_number(state, "radius", 0.0, 64.0) {
            patch.radius = radius;
            patch.fields |= PATCH_RADIUS;
        }
        if let Some(opacity) = patch_number(state, "opacity", 0.0, 1.0) {
            patch.opacity = opacity;
            patch.fields |= PATCH_OPACITY;
        }
        if let Some(padding) = patch_quad(state, "padding") {
            patch.padding = padding;
            patch.fields |= PATCH_PADDING;
        }
        if let Some(margin) = patch_quad(state, "margin") {
            patch.margin = margin;
            patch.fields |= PATCH_MARGIN;
        }
        if let Some(border) = patch_color(state, "border_color") {
            patch.border_color = border;
            if let Some(width) = patch_number(state, "border_width", 0.0, 8.0) {
                patch.border_width = width;
                patch.fields |= PATCH_BORDER;
            }
        }
        if let Some(color) = patch_color(state, "text_color") {
            patch.text_color = color;
            patch.fields |= PATCH_TEXT_COLOR;
        }
        if let Some(scale) = patch_number(state, "text_scale", 0.5, 3.0) {
            patch.text_scale = scale;
            patch.fields |= PATCH_TEXT_SCALE;
        }
        lua::get_field(state, -1, "hidden");
        if lua::lua_toboolean(state, -1) != 0 {
            patch.fields |= PATCH_HIDDEN;
        }
        lua::pop(state, 1);
    }
}

pub unsafe fn push_snowflake_field(state: *mut LuaState, field: &str, value: u64) {
    if value != 0 {
        unsafe { set_text(state, field, &value.to_string()) };
    }
}

pub unsafe fn push_target_author(state: *mut LuaState, target: &AstralPatchTarget) {
    if target.author_id != 0 || target.message_id == 0 {
        unsafe { push_snowflake_field(state, "author_id", target.author_id) };
        return;
    }
    let message = target.message_id.to_string();
    let channel = if target.channel_id != 0 {
        target.channel_id.to_string()
    } else {
        resolve_channel(0, &message)
    };
    let author = message_author(&channel, &message);
    unsafe { set_optional(state, "author_id", &author) };
}

pub unsafe fn push_target_channel(state: *mut LuaState, target: &AstralPatchTarget) {
    if target.channel_id != 0 {
        unsafe { push_snowflake_field(state, "channel_id", target.channel_id) };
        return;
    }
    if target.message_id == 0 {
        return;
    }
    if let Some(channel) = crate::discord_cache::message_channel(&target.message_id.to_string()).filter(|channel| !channel.is_empty()) {
        unsafe { set_text(state, "channel_id", &channel) };
    }
}
