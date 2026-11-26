use crate::lua::enums::TextKind;
use crate::lua::enums::{Align, InsertNodeType, InsertPosition, Justify, NamePlacement, ReplyPlacement, TimePlacement};
use crate::lua::host::text_style::{self, Field};
use core::ffi::{c_char, c_int};
use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;

use serde_json::{Value, json};

use crate::lua::api::{self as lua, LuaState, NOREF};
use crate::lua::host::hooks::{
    busy, c_text, guild_of, message_author, patch_color, patch_number, patch_quad, push_snowflake_field, push_target_author,
    push_target_channel, read_node_patch, resolve_channel, running_plugins, set_optional, set_text, snowflake, write_raw, write_text,
};
use crate::lua::host::{self as host, NodeInsert, Plugin};
use crate::lua::json::to_json;
use crate::support::bridge::AstralRustString;
use crate::wire::abi::{AstralMentionDecoration, AstralNameContext, AstralNodePatch, AstralPatchTarget, NAME_TYPING};
use crate::wire::code_blocks::AstralCodeBlockContext;
use crate::wire::decorate::{AstralDecorationContext, AstralMessageBackgroundContext, AstralMessageBackgroundStyle, AstralTag};
use crate::wire::insert::{AstralInsertNode, INSERT_IMAGE, INSERT_PLAYER, INSERT_TEXT};
use crate::wire::message_menu::AstralMenuEntry;
use crate::wire::reactions::{AstralReactionContext, AstralReactionDecoration, FACES};

const INSERT_VIEW: c_int = 2;
const INSERT_COLOR: u32 = 1 << 0;
const INSERT_BACKGROUND: u32 = 1 << 1;
const INSERT_RADIUS: u32 = 1 << 2;
const INSERT_PADDING: u32 = 1 << 3;
const INSERT_MARGIN: u32 = 1 << 4;
const INSERT_FONT_SIZE: u32 = 1 << 5;
const INSERT_WIDTH: u32 = 1 << 6;
const INSERT_HEIGHT: u32 = 1 << 7;
const INSERT_ROW: u32 = 1 << 8;
const INSERT_TINT: u32 = 1 << 9;
const INSERT_BOLD: u32 = 1 << 10;
const INSERT_JUSTIFY: u32 = 1 << 11;
const INSERT_FILL: u32 = 1 << 12;
const INSERT_MUTED: u32 = 1 << 13;
const INSERT_FILL_HEIGHT: u32 = 1 << 14;
const INSERT_ALIGN: u32 = 1 << 15;
const TAG_PILL: c_int = 0;
const TAG_ORIGINAL_POSTER: c_int = 1;
const TAG_ICON: c_int = 2;

fn plugin_icon_uri(plugin: &Plugin, icon: &str) -> Option<String> {
    if icon.len() >= 500 {
        return None;
    }
    if icon.starts_with("res://") || icon.starts_with("https://") {
        return Some(icon.to_owned());
    }
    let requested = std::path::Path::new(icon);
    if icon.is_empty() || requested.is_absolute() || requested.components().any(|part| part.as_os_str() == "..") {
        return None;
    }
    let extension = requested.extension().and_then(|extension| extension.to_str()).unwrap_or_default();
    if extension != "png" && extension != "webp" {
        return None;
    }
    let full = std::path::Path::new(&plugin.package.directory).join(requested);
    full.is_file().then(|| format!("file://{}", full.display()))
}

#[unsafe(export_name = "AstralLuaImageRewrite")]
pub unsafe extern "C" fn astral_lua_image_rewrite(uri: *const c_char, rewritten: *mut c_char, capacity: usize) -> c_int {
    if uri.is_null() || rewritten.is_null() || capacity == 0 {
        return 0;
    }
    let Some(_held) = host::try_lock() else { return busy(0) };
    let mut current = unsafe { c_text(uri) };
    let mut changed = false;
    for plugin in running_plugins() {
        let mut index = 0;
        while index < unsafe { (*plugin).image_rewriters.len() } {
            let (prefix, reference) = unsafe { (&(*plugin).image_rewriters)[index].clone() };
            index += 1;
            if !current.starts_with(&prefix) {
                continue;
            }
            let state = unsafe { (*plugin).state() };
            unsafe {
                lua::push_reference(state, reference);
                lua::push_str(state, &current);
            }
            if !host::call(plugin, 1, 1, "image rewrite") {
                host::fail_and_stop(plugin);
                break;
            }
            if unsafe { lua::lua_isstring(state, -1) } != 0
                && let Some(next) = unsafe { lua::to_bytes(state, -1) }
                && !next.is_empty()
                && next.len() < capacity
                && next.starts_with(b"http")
                && !next.iter().any(|byte| byte.is_ascii_control() || *byte == b' ')
            {
                current = String::from_utf8_lossy(next).into_owned();
                changed = true;
            }
            unsafe { lua::pop(state, 1) };
        }
    }
    if !changed {
        return 0;
    }
    unsafe { write_raw(rewritten, capacity, &current) };
    1
}

unsafe fn push_patch_target(state: *mut LuaState, target: &AstralPatchTarget, prefix_length: usize, extended: bool) {
    let identifier = unsafe { c_text(target.identifier) };
    unsafe {
        lua::lua_createtable(state, 0, 6);
        set_text(state, "id", &identifier);
        set_text(state, "key", identifier.get(prefix_length..).unwrap_or_default());
        set_text(state, "type", &c_text(target.kind));
        push_snowflake_field(state, "message_id", target.message_id);
        push_target_author(state, target);
        push_target_channel(state, target);
    }
    if !extended {
        return;
    }
    unsafe {
        if !target.parent.is_null() {
            set_text(state, "parent_id", &c_text(target.parent));
        }
        if target.node != 0 {
            lua::lua_pushinteger(state, i64::from(target.node));
            lua::set_field(state, -2, "node");
        }
        if target.checked >= 0 {
            lua::lua_pushboolean(state, target.checked);
            lua::set_field(state, -2, "checked");
        }
        if !target.label.is_null() {
            set_text(state, "label", &c_text(target.label));
        }
    }
}

#[unsafe(export_name = "AstralLuaPatchNode")]
pub unsafe extern "C" fn astral_lua_patch_node(target: *const AstralPatchTarget, patch: *mut AstralNodePatch) -> c_int {
    let (Some(target), Some(patch)) = (unsafe { target.as_ref() }, unsafe { patch.as_mut() }) else {
        return 0;
    };
    if target.identifier.is_null() || target.kind.is_null() {
        return 0;
    }
    let Some(_held) = host::try_lock() else { return busy(-1) };
    let identifier = unsafe { c_text(target.identifier) };
    let kind = unsafe { c_text(target.kind) };
    for plugin in running_plugins() {
        let patches: Vec<(usize, c_int)> = unsafe { &*plugin }
            .node_patches
            .iter()
            .filter(|entry| identifier.starts_with(&entry.prefix) && (entry.kind.is_empty() || entry.kind == kind))
            .map(|entry| (entry.prefix.len(), entry.reference))
            .collect();
        for (prefix_length, reference) in patches {
            if !unsafe { (*plugin).running() } {
                break;
            }
            let state = unsafe { (*plugin).state() };
            let top = unsafe { lua::lua_gettop(state) };
            unsafe {
                lua::push_reference(state, reference);
                push_patch_target(state, target, prefix_length, false);
            }
            if !host::call(plugin, 1, 1, "node patch") {
                unsafe { lua::lua_settop(state, top) };
                host::fail_and_stop(plugin);
                break;
            }
            if unsafe { lua::is_table(state, -1) } {
                unsafe { read_node_patch(state, patch) };
            }
            unsafe { lua::lua_settop(state, top) };
        }
    }
    c_int::from(patch.fields != 0)
}

unsafe fn copy_field(state: *mut LuaState, field: &str, out: &mut [c_char]) {
    unsafe { lua::get_field(state, -1, field) };
    if unsafe { lua::lua_isstring(state, -1) } != 0 {
        write_text(
            out,
            &String::from_utf8_lossy(unsafe { lua::to_bytes(state, -1) }.unwrap_or_default()),
        );
    }
    unsafe { lua::pop(state, 1) };
}

unsafe fn field_text(state: *mut LuaState, field: &str) -> Option<String> {
    unsafe { lua::get_field(state, -1, field) };
    let value = if unsafe { lua::lua_isstring(state, -1) } != 0 {
        Some(String::from_utf8_lossy(unsafe { lua::to_bytes(state, -1) }.unwrap_or_default()).into_owned())
    } else {
        None
    };
    unsafe { lua::pop(state, 1) };
    value
}

unsafe fn field_flag(state: *mut LuaState, field: &str) -> bool {
    unsafe { lua::get_field(state, -1, field) };
    let flag = unsafe { lua::lua_toboolean(state, -1) } != 0;
    unsafe { lua::pop(state, 1) };
    flag
}

fn c_string_length(text: &[c_char]) -> usize {
    text.iter().position(|byte| *byte == 0).unwrap_or(text.len())
}

unsafe fn read_insert_style(state: *mut LuaState, node: &mut AstralInsertNode) {
    unsafe {
        if let Some(color) = patch_color(state, "color") {
            node.color = color;
            node.fields |= if node.kind == INSERT_IMAGE { INSERT_TINT } else { INSERT_COLOR };
        }
        if let Some(background) = patch_color(state, "background") {
            node.background = background;
            node.fields |= INSERT_BACKGROUND;
        }
        if let Some(radius) = patch_number(state, "radius", 0.0, 64.0) {
            node.radius = radius;
            node.fields |= INSERT_RADIUS;
        }
        if let Some(padding) = patch_quad(state, "padding") {
            node.padding = padding;
            node.fields |= INSERT_PADDING;
        }
        if let Some(margin) = patch_quad(state, "margin") {
            node.margin = margin;
            node.fields |= INSERT_MARGIN;
        }
        if let Some(size) = patch_number(state, "size", 6.0, 48.0) {
            node.font_size = size;
            node.fields |= INSERT_FONT_SIZE;
        }
        if let Some(width) = patch_number(state, "width", 0.0, 1000.0) {
            node.width = width;
            node.fields |= INSERT_WIDTH;
        }
        if let Some(height) = patch_number(state, "height", 0.0, 1000.0) {
            node.height = height;
            node.fields |= INSERT_HEIGHT;
        }
        if field_flag(state, "row") {
            node.fields |= INSERT_ROW;
        }
        if field_flag(state, "fill") {
            node.fields |= INSERT_FILL;
            node.fields &= !INSERT_WIDTH;
        }
        if field_flag(state, "fill_height") {
            node.fields |= INSERT_FILL_HEIGHT;
            node.fields &= !INSERT_HEIGHT;
        }
        if node.kind == INSERT_PLAYER && field_flag(state, "muted") {
            node.fields |= INSERT_MUTED;
        }
        if let Some(justify) = field_text(state, "justify").as_deref().and_then(Justify::parse) {
            node.justify = justify;
            node.fields |= INSERT_JUSTIFY;
        }
        if let Some(align) = field_text(state, "align").as_deref().and_then(Align::parse) {
            node.align = align;
            node.fields |= INSERT_ALIGN;
        }
        if field_flag(state, "bold") {
            node.fields |= INSERT_BOLD;
        }
        if let Some(weight) = patch_number(state, "weight", 100.0, 900.0) {
            node.font_weight = (weight / 100.0).round() * 100.0;
            node.fields |= INSERT_BOLD;
        }
    }
}

struct InsertContext<'a> {
    plugin: &'a Plugin,
    identifier: u64,
    press: c_int,
    key: &'a str,
}

unsafe fn read_insert_node(
    state: *mut LuaState,
    context: &InsertContext,
    nodes: &mut [AstralInsertNode],
    count: &mut usize,
    parent: c_int,
) {
    if !unsafe { lua::is_table(state, -1) } || *count >= nodes.len() {
        return;
    }
    let node = &mut nodes[*count];
    unsafe { core::ptr::write_bytes(node as *mut AstralInsertNode, 0, 1) };
    node.kind = match unsafe { field_text(state, "type") }.as_deref().and_then(InsertNodeType::parse) {
        Some(InsertNodeType::Text) => INSERT_TEXT,
        Some(InsertNodeType::Image) => INSERT_IMAGE,
        Some(InsertNodeType::Player) => INSERT_PLAYER,
        Some(InsertNodeType::View) | None => INSERT_VIEW,
    };
    node.parent = parent;
    unsafe { copy_field(state, "text", &mut node.text) };
    if let Some(icon) = unsafe { field_text(state, "icon") }.filter(|icon| !icon.is_empty() && icon.len() < 256)
        && let Some(uri) = plugin_icon_uri(context.plugin, &icon).filter(|uri| uri.len() < node.uri.len())
    {
        write_text(&mut node.uri, &uri);
    }
    if node.kind == INSERT_PLAYER
        && let Some(video) = unsafe { field_text(state, "uri") }
            .filter(|video| video.len() < 256 && (video.starts_with("content://") || video.starts_with("https://")))
    {
        write_text(&mut node.uri, &video);
    }
    if let Some(action) = unsafe { field_text(state, "action") }.filter(|action| !action.is_empty() && action.len() < 32)
        && context.press != NOREF
    {
        write_text(
            &mut node.tap,
            &format!("astral.tap.{}:{action}:{}", context.identifier, context.key),
        );
        if c_string_length(&node.tap) >= node.tap.len() - 1 {
            node.tap[0] = 0;
        }
    }
    unsafe { read_insert_style(state, node) };
    let index = *count as c_int;
    *count += 1;
    unsafe { lua::get_field(state, -1, "children") };
    if unsafe { lua::is_table(state, -1) } {
        let children = unsafe { lua::luaL_len(state, -1) };
        let mut child = 1;
        while child <= children && *count < nodes.len() {
            unsafe { lua::lua_rawgeti(state, -1, child) };
            unsafe { read_insert_node(state, context, nodes, count, index) };
            unsafe { lua::pop(state, 1) };
            child += 1;
        }
    }
    unsafe { lua::pop(state, 1) };
}

#[unsafe(export_name = "AstralLuaInsertNodes")]
pub unsafe extern "C" fn astral_lua_insert_nodes(
    target: *const AstralPatchTarget,
    nodes: *mut AstralInsertNode,
    capacity: usize,
    at_start: *mut c_int,
) -> c_int {
    let Some(target) = (unsafe { target.as_ref() }) else { return 0 };
    if target.identifier.is_null() || target.kind.is_null() || nodes.is_null() || at_start.is_null() {
        return 0;
    }
    let Some(_held) = host::try_lock() else { return busy(-1) };
    let nodes = unsafe { core::slice::from_raw_parts_mut(nodes, capacity) };
    let identifier = unsafe { c_text(target.identifier) };
    let kind = unsafe { c_text(target.kind) };
    let mut count = 0;
    for plugin in running_plugins() {
        let inserts: Vec<(u64, usize, c_int, c_int)> = unsafe { &*plugin }
            .node_inserts
            .iter()
            .filter(|entry: &&NodeInsert| identifier.starts_with(&entry.prefix) && (entry.kind.is_empty() || entry.kind == kind))
            .map(|entry| (entry.identifier, entry.prefix.len(), entry.build, entry.press))
            .collect();
        for (registration, prefix_length, build, press) in inserts {
            if !unsafe { (*plugin).running() } {
                break;
            }
            let state = unsafe { (*plugin).state() };
            let top = unsafe { lua::lua_gettop(state) };
            unsafe {
                lua::push_reference(state, build);
                push_patch_target(state, target, prefix_length, true);
            }
            if !host::call(plugin, 1, 1, "node insert") {
                unsafe { lua::lua_settop(state, top) };
                host::fail_and_stop(plugin);
                break;
            }
            let key = identifier.get(prefix_length..).unwrap_or_default().to_owned();
            if unsafe { lua::is_table(state, -1) } {
                if unsafe { field_text(state, "position") }.as_deref().and_then(InsertPosition::parse) == Some(InsertPosition::Start) {
                    unsafe { *at_start = 1 };
                }
                unsafe { lua::get_field(state, -1, "type") };
                let single = unsafe { lua::lua_type(state, -1) } != lua::TNIL;
                unsafe { lua::pop(state, 1) };
                let context = InsertContext {
                    plugin: unsafe { &*plugin },
                    identifier: registration,
                    press,
                    key: &key,
                };
                if single {
                    unsafe { read_insert_node(state, &context, nodes, &mut count, -1) };
                } else {
                    let items = unsafe { lua::luaL_len(state, -1) };
                    let mut item = 1;
                    while item <= items && count < nodes.len() {
                        unsafe { lua::lua_rawgeti(state, -1, item) };
                        unsafe { read_insert_node(state, &context, nodes, &mut count, -1) };
                        unsafe { lua::pop(state, 1) };
                        item += 1;
                    }
                }
            }
            unsafe { lua::lua_settop(state, top) };
        }
    }
    count as c_int
}

#[unsafe(export_name = "AstralLuaCodeBlock")]
pub unsafe extern "C" fn astral_lua_code_block(context: *const AstralCodeBlockContext, card: *mut AstralRustString) -> c_int {
    let (Some(context), Some(card)) = (unsafe { context.as_ref() }, unsafe { card.as_mut() }) else {
        return 0;
    };
    let Some(_held) = host::try_lock() else { return busy(-1) };
    let message = snowflake(context.message_id);
    let channel = snowflake(context.channel_id);
    let code = String::from_utf8_lossy(crate::support::bridge::bytes(context.code, context.code_length)).into_owned();
    let key = format!("{message}.{}", context.index);
    for plugin in running_plugins() {
        let blocks: Vec<(u64, c_int, c_int)> = unsafe { &*plugin }
            .code_blocks
            .iter()
            .map(|entry| (entry.identifier, entry.render, entry.press))
            .collect();
        for (identifier, render, press) in blocks {
            let state = unsafe { (*plugin).state() };
            let top = unsafe { lua::lua_gettop(state) };
            unsafe {
                lua::push_reference(state, render);
                lua::lua_createtable(state, 0, 4);
                set_text(state, "message_id", &message);
                set_optional(state, "channel_id", &channel);
                lua::lua_pushinteger(state, i64::from(context.index));
                lua::set_field(state, -2, "index");
                set_text(state, "code", &code);
            }
            if !host::call(plugin, 1, 1, "code block") {
                unsafe { lua::lua_settop(state, top) };
                host::fail_and_stop(plugin);
                return 0;
            }
            let rendered = if unsafe { lua::is_table(state, -1) } {
                unsafe { to_json(state, -1, 0) }.filter(Value::is_object)
            } else {
                None
            };
            unsafe { lua::lua_settop(state, top) };
            let Some(mut rendered) = rendered else { continue };
            if press != NOREF
                && rendered
                    .get("header")
                    .is_some_and(|header| header.is_object() && header.get("copy").is_some())
            {
                rendered["header"]["tap"] = json!(format!("astral.tap.{identifier}:copy:{key}"));
                let codes = &mut host::host().code_block_code;
                if codes.len() > 256 {
                    codes.clear();
                }
                codes.insert(key.clone(), code.clone());
            }
            *card = AstralRustString::new(rendered.to_string().into_bytes());
            return 1;
        }
    }
    0
}

#[unsafe(export_name = "AstralLuaMessageBackground")]
pub unsafe extern "C" fn astral_lua_message_background(
    context: *const AstralMessageBackgroundContext,
    style: *mut AstralMessageBackgroundStyle,
) -> c_int {
    let (Some(context), Some(style)) = (unsafe { context.as_ref() }, unsafe { style.as_mut() }) else {
        return 0;
    };
    let Some(_held) = host::try_lock() else { return busy(-1) };
    if !running_plugins()
        .into_iter()
        .any(|plugin| unsafe { !(*plugin).message_background_decorators.is_empty() })
    {
        return 0;
    }
    let message = snowflake(context.message_id);
    let channel = snowflake(context.channel_id);
    let mut user = snowflake(context.user_id);
    if user.is_empty() {
        let resolved = if channel.is_empty() {
            resolve_channel(0, &message)
        } else {
            channel.clone()
        };
        user = message_author(&resolved, &message);
    }
    for plugin in running_plugins() {
        let decorators = unsafe { &*plugin }.message_background_decorators.clone();
        for reference in decorators {
            let state = unsafe { (*plugin).state() };
            let top = unsafe { lua::lua_gettop(state) };
            unsafe {
                lua::push_reference(state, reference);
                lua::lua_createtable(state, 0, 3);
                set_optional(state, "message_id", &message);
                set_optional(state, "author_id", &user);
                set_optional(state, "channel_id", &channel);
            }
            if !host::call(plugin, 1, 1, "message background") {
                unsafe { lua::lua_settop(state, top) };
                host::fail_and_stop(plugin);
                break;
            }
            let mut found = false;
            if unsafe { lua::is_table(state, -1) } {
                let color = unsafe { patch_color(state, "color") };
                unsafe { lua::get_field(state, -1, "radius") };
                let radius = if unsafe { lua::lua_isnumber(state, -1) } != 0 {
                    unsafe { lua::to_number(state, -1) }
                } else {
                    -1.0
                };
                unsafe { lua::pop(state, 1) };
                let bubble = unsafe { field_flag(state, "bubble") };
                let name_inside =
                    unsafe { field_text(state, "name") }.as_deref().and_then(NamePlacement::parse) == Some(NamePlacement::Inside);
                let time = unsafe { field_text(state, "time") }
                    .as_deref()
                    .and_then(TimePlacement::parse)
                    .unwrap_or_default();
                let reply = unsafe { field_text(state, "reply") }
                    .as_deref()
                    .and_then(ReplyPlacement::parse)
                    .unwrap_or_default();
                if let Some(color) = color
                    && radius.is_finite()
                    && (0.0..=32.0).contains(&radius)
                {
                    style.color = color;
                    style.radius = radius;
                    style.bubble = c_int::from(bubble);
                    style.reply = reply;
                    style.name_inside = c_int::from(name_inside);
                    style.time = time;
                    found = true;
                }
            }
            unsafe { lua::lua_settop(state, top) };
            if found {
                return 1;
            }
        }
        if unsafe { (*plugin).failed } {
            host::stop_plugin(plugin);
        }
    }
    0
}

#[unsafe(export_name = "AstralLuaTyping")]
pub unsafe extern "C" fn astral_lua_typing(context: *const AstralNameContext, decoration: *mut AstralMentionDecoration) -> c_int {
    let (Some(context), Some(decoration)) = (unsafe { context.as_ref() }, unsafe { decoration.as_mut() }) else {
        return 0;
    };
    let Some(_held) = host::try_lock() else { return busy(0) };
    if !text_style::wanted() {
        return 0;
    }
    let channel = snowflake(context.channel_id);
    let text = unsafe { c_text(context.text) };
    let Some(guild) = guild_of(&channel) else { return 0 };
    let Some(user) = crate::discord::cache::member_named(&guild, &text) else {
        return 0;
    };
    let fields = [
        ("text", Field::Text(&text)),
        ("user_id", Field::Text(&user)),
        ("channel_id", Field::Text(&channel)),
        ("guild_id", Field::Text(&guild)),
    ];
    let Some((image, round)) = text_style::style(TextKind::Typing, &fields).and_then(|style| style.image.zip(style.round)) else {
        return 0;
    };
    if !image.starts_with("https://") {
        return 0;
    }
    write_text(&mut decoration.image, &image);
    decoration.round = c_int::from(round);
    1
}

unsafe fn read_reaction_images(state: *mut LuaState, decoration: &mut AstralReactionDecoration) {
    unsafe { lua::get_field(state, -1, "images") };
    if unsafe { lua::is_table(state, -1) } {
        let length = unsafe { lua::lua_rawlen(state, -1) } as i64;
        let mut index = 1;
        while index <= length && (decoration.image_count as usize) < FACES {
            unsafe { lua::lua_rawgeti(state, -1, index) };
            let image = unsafe { lua::to_bytes(state, -1) }
                .map(|image| String::from_utf8_lossy(image).into_owned())
                .unwrap_or_default();
            if image.starts_with("https://") {
                write_text(&mut decoration.images[decoration.image_count as usize], &image);
                decoration.image_count += 1;
            }
            unsafe { lua::pop(state, 1) };
            index += 1;
        }
    }
    unsafe { lua::pop(state, 1) };
    unsafe { lua::get_field(state, -1, "total") };
    if unsafe { lua::lua_isinteger(state, -1) } != 0 {
        decoration.total = unsafe { lua::to_integer(state, -1) }.max(0) as u32;
    }
    unsafe { lua::pop(state, 1) };
}

#[unsafe(export_name = "AstralLuaReaction")]
pub unsafe extern "C" fn astral_lua_reaction(context: *const AstralReactionContext, decoration: *mut AstralReactionDecoration) -> c_int {
    let (Some(context), Some(decoration)) = (unsafe { context.as_ref() }, unsafe { decoration.as_mut() }) else {
        return 0;
    };
    let Some(_held) = host::try_lock() else { return busy(0) };
    let channel = snowflake(context.channel_id);
    let message = snowflake(context.message_id);
    if channel.is_empty() || message.is_empty() {
        return 0;
    }
    let guild = crate::discord::cache::channel_guild(&channel);
    let emoji = unsafe { c_text(context.emoji) };
    for plugin in running_plugins() {
        let decorators = unsafe { &*plugin }.reaction_decorators.clone();
        for reference in decorators {
            let state = unsafe { (*plugin).state() };
            unsafe {
                lua::push_reference(state, reference);
                lua::lua_createtable(state, 0, 6);
                set_text(state, "channel_id", &channel);
                set_text(state, "message_id", &message);
                if let Some(guild) = &guild {
                    set_text(state, "guild_id", guild);
                }
                set_text(state, "emoji", &emoji);
                lua::lua_pushboolean(state, context.burst);
                lua::set_field(state, -2, "burst");
                lua::lua_pushinteger(state, i64::from(context.count));
                lua::set_field(state, -2, "count");
            }
            if host::call(plugin, 1, 1, "reaction") {
                if unsafe { lua::is_table(state, -1) } {
                    unsafe { read_reaction_images(state, decoration) };
                }
                unsafe { lua::pop(state, 1) };
                decoration.stale = 0;
                if decoration.image_count != 0 {
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

#[unsafe(export_name = "AstralLuaNameColor")]
pub unsafe extern "C" fn astral_lua_name_color(context: *const AstralNameContext, color: *mut u32) -> c_int {
    let Some(context) = (unsafe { context.as_ref() }) else { return 0 };
    let Some(_held) = host::try_lock() else { return busy(0) };
    if !text_style::wanted() {
        return 0;
    }
    let channel = snowflake(context.channel_id);
    let Some(guild) = guild_of(&channel) else { return 0 };
    let text = unsafe { c_text(context.text) };
    let mut user = snowflake(context.user_id);
    if user.is_empty() && context.kind == NAME_TYPING {
        let Some(named) = crate::discord::cache::member_named(&guild, &text) else {
            return 0;
        };
        user = named;
    }
    let fields = [
        ("text", Field::Text(&text)),
        ("user_id", Field::Text(&user)),
        ("channel_id", Field::Text(&channel)),
        ("guild_id", Field::Text(&guild)),
    ];
    let Some(found) = text_style::style(TextKind::from_wire(context.kind), &fields).and_then(|style| style.color) else {
        return 0;
    };
    if let Some(color) = unsafe { color.as_mut() } {
        *color = found;
    }
    1
}

const REMEMBERED_TAG_ROWS: usize = 512;

struct TagMemory {
    rows: BTreeMap<(u64, u64), Vec<AstralTag>>,
    order: VecDeque<(u64, u64)>,
}

static TAG_MEMORY: Mutex<TagMemory> = Mutex::new(TagMemory {
    rows: BTreeMap::new(),
    order: VecDeque::new(),
});

fn remember_tags(context: &AstralDecorationContext, tags: &[AstralTag]) {
    let mut memory = TAG_MEMORY.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let row = (context.message_id, context.user_id);
    if !memory.rows.contains_key(&row) {
        memory.order.push_back(row);
        if memory.order.len() > REMEMBERED_TAG_ROWS
            && let Some(oldest) = memory.order.pop_front()
        {
            memory.rows.remove(&oldest);
        }
    }
    memory.rows.insert(row, tags.to_vec());
}

fn remembered_tags(context: &AstralDecorationContext, tags: &mut [AstralTag]) -> Option<usize> {
    let memory = TAG_MEMORY.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let found = memory.rows.get(&(context.message_id, context.user_id))?;
    let count = tags.len().min(found.len());
    tags[..count].copy_from_slice(&found[..count]);
    Some(count)
}

unsafe fn collect_tags(state: *mut LuaState, plugin: &Plugin, tags: &mut [AstralTag], count: &mut usize) {
    if !unsafe { lua::is_table(state, -1) } {
        unsafe { lua::pop(state, 1) };
        return;
    }
    let length = unsafe { lua::luaL_len(state, -1) };
    let mut index = 1;
    while index <= length && *count < tags.len() {
        unsafe { lua::lua_rawgeti(state, -1, index) };
        index += 1;
        if unsafe { lua::is_table(state, -1) } {
            let icon = unsafe { field_text(state, "icon") }.and_then(|icon| plugin_icon_uri(plugin, &icon));
            if let Some(icon) = icon {
                let tag = &mut tags[*count];
                *count += 1;
                unsafe { core::ptr::write_bytes(tag as *mut AstralTag, 0, 1) };
                tag.shape = TAG_ICON;
                write_text(&mut tag.icon, &icon);
                unsafe { lua::get_field(state, -1, "tint") };
                if unsafe { lua::lua_isinteger(state, -1) } != 0 {
                    tag.tint = unsafe { lua::to_integer(state, -1) } as u32;
                }
                unsafe { lua::pop(state, 1) };
                if let Some(text) = unsafe { field_text(state, "text") } {
                    write_text(&mut tag.text, &text);
                }
                unsafe { lua::pop(state, 1) };
                continue;
            }
            let text = unsafe { field_text(state, "text") }.filter(|text| !text.is_empty());
            if let Some(text) = text {
                let tag = &mut tags[*count];
                *count += 1;
                unsafe { core::ptr::write_bytes(tag as *mut AstralTag, 0, 1) };
                write_text(&mut tag.text, &text);
                unsafe {
                    lua::get_field(state, -1, "color");
                    tag.background = if lua::lua_isinteger(state, -1) != 0 {
                        lua::to_integer(state, -1) as u32
                    } else {
                        0xFF58_65F2
                    };
                    lua::pop(state, 1);
                    lua::get_field(state, -1, "text_color");
                    tag.foreground = if lua::lua_isinteger(state, -1) != 0 {
                        lua::to_integer(state, -1) as u32
                    } else {
                        0xFFFF_FFFF
                    };
                    lua::pop(state, 1);
                    lua::get_field(state, -1, "colors");
                    if lua::is_table(state, -1) {
                        for stop in 1..=3 {
                            lua::lua_rawgeti(state, -1, stop);
                            if lua::lua_isinteger(state, -1) != 0 {
                                tag.gradient[tag.gradient_count as usize] = lua::to_integer(state, -1) as u32;
                                tag.gradient_count += 1;
                            }
                            lua::pop(state, 1);
                        }
                    }
                    lua::pop(state, 1);
                }
                tag.shape = if unsafe { field_text(state, "shape") }.as_deref() == Some("op") {
                    TAG_ORIGINAL_POSTER
                } else {
                    TAG_PILL
                };
            }
        }
        unsafe { lua::pop(state, 1) };
    }
    unsafe { lua::pop(state, 1) };
}

fn decorate_with_lua(context: &AstralDecorationContext, tags: &mut [AstralTag]) -> usize {
    let message = snowflake(context.message_id);
    let channel = resolve_channel(context.channel_id, &message);
    let mut user = snowflake(context.user_id);
    if user.is_empty() {
        user = message_author(&channel, &message);
    }
    let guild = guild_of(&channel);
    let mut count = 0;
    for plugin in running_plugins() {
        let decorators: Vec<c_int> = unsafe { &*plugin }.decorators.iter().map(|(_, reference)| *reference).collect();
        for reference in decorators {
            if count >= tags.len() {
                break;
            }
            let state = unsafe { (*plugin).state() };
            unsafe {
                lua::push_reference(state, reference);
                lua::lua_createtable(state, 0, 4);
                set_text(state, "message_id", &message);
                set_optional(state, "user_id", &user);
                set_optional(state, "channel_id", &channel);
                if let Some(guild) = &guild {
                    set_text(state, "guild_id", guild);
                }
            }
            if host::call(plugin, 1, 1, "decorate") {
                unsafe { collect_tags(state, &*plugin, tags, &mut count) };
            }
            if unsafe { (*plugin).failed } {
                break;
            }
        }
        if unsafe { (*plugin).failed } {
            host::stop_plugin(plugin);
        }
    }
    count
}

#[unsafe(export_name = "AstralLuaDecorate")]
pub unsafe extern "C" fn astral_lua_decorate(context: *const AstralDecorationContext, tags: *mut AstralTag, capacity: usize) -> usize {
    let Some(context) = (unsafe { context.as_ref() }) else { return 0 };
    if tags.is_null() {
        return 0;
    }
    let tags = unsafe { core::slice::from_raw_parts_mut(tags, capacity) };
    let Some(_held) = host::try_lock() else {
        if let Some(remembered) = remembered_tags(context, tags) {
            return remembered;
        }
        return busy(0);
    };
    let count = decorate_with_lua(context, tags);
    remember_tags(context, &tags[..count]);
    count
}

pub fn set_menu_message(channel: u64, message: u64) {
    let _held = host::lock();
    let host = host::host();
    host.menu_channel = channel;
    host.menu_message = message;
}

pub fn press_inserted_node(identifier: &str) -> bool {
    let _held = host::lock();
    let Some(rest) = identifier.strip_prefix("astral.tap.") else {
        return false;
    };
    let Some((registration, rest)) = rest.split_once(':') else {
        return false;
    };
    let Some((action, key)) = rest.split_once(':') else { return false };
    if registration.is_empty() || !registration.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    let Ok(registration) = registration.parse::<u64>() else {
        return false;
    };
    for plugin in running_plugins() {
        let Some((prefix, kind, press)) = (unsafe { &*plugin })
            .node_inserts
            .iter()
            .find(|entry| entry.identifier == registration && entry.press != NOREF)
            .map(|entry| (entry.prefix.clone(), entry.kind.clone(), entry.press))
        else {
            continue;
        };
        let full = std::ffi::CString::new(format!("{prefix}{key}")).unwrap_or_default();
        let kind = std::ffi::CString::new(kind).unwrap_or_default();
        let message = unsafe { crate::wire::patch::astral_wire_message_of(full.as_ptr()) };
        let target = AstralPatchTarget {
            identifier: full.as_ptr(),
            kind: kind.as_ptr(),
            key: prefix.len(),
            message_id: message,
            author_id: 0,
            channel_id: crate::wire::ids::astral_wire_current_channel(),
            parent: core::ptr::null(),
            node: 0,
            checked: -1,
            label: core::ptr::null(),
        };
        let state = unsafe { (*plugin).state() };
        let top = unsafe { lua::lua_gettop(state) };
        unsafe {
            lua::push_reference(state, press);
            push_patch_target(state, &target, prefix.len(), true);
            lua::push_str(state, action);
        }
        if !host::call(plugin, 2, 0, "insert press") {
            unsafe { lua::lua_settop(state, top) };
            host::fail_and_stop(plugin);
            return true;
        }
        unsafe { lua::lua_settop(state, top) };
        return true;
    }
    for plugin in running_plugins() {
        let Some(press) = (unsafe { &*plugin })
            .code_blocks
            .iter()
            .find(|entry| entry.identifier == registration && entry.press != NOREF)
            .map(|entry| entry.press)
        else {
            continue;
        };
        let code = host::host().code_block_code.get(key).cloned().unwrap_or_default();
        let (message, index) = match key.rsplit_once('.') {
            Some((message, index)) => (message.to_owned(), index.parse::<i64>().unwrap_or(0)),
            None => (key.to_owned(), 0),
        };
        let state = unsafe { (*plugin).state() };
        let top = unsafe { lua::lua_gettop(state) };
        unsafe {
            lua::push_reference(state, press);
            lua::lua_createtable(state, 0, 3);
            set_text(state, "message_id", &message);
            lua::lua_pushinteger(state, index);
            lua::set_field(state, -2, "index");
            set_text(state, "code", &code);
            lua::push_str(state, action);
        }
        if !host::call(plugin, 2, 0, "code block press") {
            unsafe { lua::lua_settop(state, top) };
            host::fail_and_stop(plugin);
            return true;
        }
        unsafe { lua::lua_settop(state, top) };
        return true;
    }
    false
}

pub fn press_message_action(identifier: &str) -> bool {
    let _held = host::lock();
    let Some(rest) = identifier.strip_prefix("astral.action.") else {
        return false;
    };
    if rest.is_empty() || !rest.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    let Ok(action) = rest.parse::<u64>() else { return false };
    if action == 0 {
        return false;
    }
    let (channel_id, message_id) = (host::host().menu_channel, host::host().menu_message);
    if channel_id == 0 || message_id == 0 {
        host::developer_log(&format!("Failed to press message action {action}: no menu message"));
        return false;
    }
    let Some((owner, reference)) = running_plugins().into_iter().find_map(|plugin| {
        unsafe { &*plugin }
            .message_actions
            .iter()
            .find(|entry| entry.identifier == action)
            .map(|entry| (plugin, entry.reference))
    }) else {
        return false;
    };
    let channel = channel_id.to_string();
    let message = message_id.to_string();
    let record = crate::discord::cache::message(&channel, &message);
    if record.is_none() {
        host::developer_log(&format!("Message action {action} found no cached message {message}"));
    }
    let guild = crate::discord::cache::channel_guild(&channel);
    let user = record
        .as_ref()
        .and_then(|record| {
            record
                .get("author")
                .and_then(|author| author.get("id"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_default();
    let state = unsafe { (*owner).state() };
    unsafe {
        lua::push_reference(state, reference);
        lua::lua_createtable(state, 0, 4);
        set_text(state, "message_id", &message);
        set_text(state, "channel_id", &channel);
        set_optional(state, "user_id", &user);
        if let Some(guild) = &guild {
            set_text(state, "guild_id", guild);
        }
    }
    host::call_or_stop(owner, 1, 0, "message.action");
    true
}

fn action_shows(plugin: *mut Plugin, when: c_int) -> bool {
    if when == NOREF {
        return true;
    }
    let (menu_channel, menu_message) = (host::host().menu_channel, host::host().menu_message);
    if menu_message == 0 {
        return false;
    }
    let message = menu_message.to_string();
    let channel = if menu_channel != 0 {
        Some(menu_channel.to_string())
    } else {
        crate::discord::cache::message_channel(&message)
    };
    let Some(channel) = channel.filter(|channel| !channel.is_empty()) else {
        return false;
    };
    let state = unsafe { (*plugin).state() };
    let top = unsafe { lua::lua_gettop(state) };
    unsafe {
        lua::push_reference(state, when);
        lua::lua_createtable(state, 0, 2);
        set_text(state, "message_id", &message);
        set_text(state, "channel_id", &channel);
    }
    if !host::call(plugin, 1, 1, "message.action when") {
        unsafe { lua::lua_settop(state, top) };
        return false;
    }
    let shows = unsafe { lua::lua_toboolean(state, -1) } != 0;
    unsafe { lua::lua_settop(state, top) };
    shows
}

#[unsafe(export_name = "AstralLuaMessageMenu")]
pub unsafe extern "C" fn astral_lua_message_menu(entries: *mut AstralMenuEntry, capacity: usize) -> usize {
    if entries.is_null() {
        return 0;
    }
    let Some(_held) = host::try_lock() else { return busy(0) };
    let entries = unsafe { core::slice::from_raw_parts_mut(entries, capacity) };
    let mut count = 0;
    for plugin in running_plugins() {
        let actions: Vec<(u64, String, String, c_int)> = unsafe { &*plugin }
            .message_actions
            .iter()
            .map(|action| (action.identifier, action.label.clone(), action.icon.clone(), action.when))
            .collect();
        for (identifier, label, icon, when) in actions {
            if count >= capacity {
                return count;
            }
            if !action_shows(plugin, when) {
                continue;
            }
            let entry = &mut entries[count];
            count += 1;
            write_text(&mut entry.action_id, &format!("astral.action.{identifier}"));
            write_text(&mut entry.label, &label);
            write_text(&mut entry.icon, &icon);
        }
    }
    count
}

#[unsafe(export_name = "AstralLuaMessageText")]
pub unsafe extern "C" fn astral_lua_message_text(
    message_id: u64,
    text: *mut c_char,
    text_capacity: usize,
    note: *mut c_char,
    note_capacity: usize,
) -> c_int {
    let Some(_held) = host::try_lock() else { return busy(0) };
    let Some((displayed, displayed_note)) = host::host().displayed_texts.get(&message_id).cloned() else {
        return 0;
    };
    unsafe {
        write_raw(text, text_capacity, &displayed);
        write_raw(note, note_capacity, &displayed_note);
    }
    1
}

#[unsafe(export_name = "AstralLuaSendTransform")]
pub unsafe extern "C" fn astral_lua_send_transform(channel_id: u64, text: *const c_char, rewritten: *mut c_char, capacity: usize) -> c_int {
    let content = unsafe { c_text(text) };
    let Some(changed) = crate::lua::host::events::transform_outgoing_message(&snowflake(channel_id), "send", &content) else {
        return 0;
    };
    if changed.len() >= capacity {
        return 0;
    }
    unsafe { write_raw(rewritten, capacity, &changed) };
    1
}
