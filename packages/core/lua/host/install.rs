use core::ffi::{c_int, c_void};
use std::sync::OnceLock;

use crate::lua::api::{self as lua, CFunction, LuaState};
use crate::lua::host::api::{self as api, guarded};
use crate::lua::host::http;
use crate::lua::host::ui;
use crate::lua::host::{self as host, Plugin};

const PRELUDE_SOURCE: &str = include_str!("../prelude.lua");

static PRELUDE: OnceLock<Vec<u8>> = OnceLock::new();

unsafe extern "C" fn collect_chunk(_: *mut LuaState, data: *const c_void, size: usize, into: *mut c_void) -> c_int {
    let into = unsafe { &mut *into.cast::<Vec<u8>>() };
    into.extend_from_slice(unsafe { core::slice::from_raw_parts(data.cast::<u8>(), size) });
    0
}

fn compile_prelude() -> Vec<u8> {
    let state = unsafe { lua::luaL_newstate() };
    if state.is_null() {
        return Vec::new();
    }
    let mut bytecode = Vec::new();
    let loaded = unsafe {
        lua::luaL_loadbufferx(
            state,
            PRELUDE_SOURCE.as_ptr().cast(),
            PRELUDE_SOURCE.len(),
            c"=astral".as_ptr(),
            c"t".as_ptr(),
        )
    };
    if loaded == lua::OK {
        unsafe { lua::lua_dump(state, collect_chunk, (&mut bytecode as *mut Vec<u8>).cast(), 1) };
    }
    unsafe { lua::lua_close(state) };
    bytecode
}

pub fn prelude() -> &'static [u8] {
    PRELUDE.get_or_init(compile_prelude)
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn encode_base64(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let value = (u32::from(chunk[0]) << 16) | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8) | u32::from(*chunk.get(2).unwrap_or(&0));
        for index in 0..4 {
            if index <= chunk.len() {
                encoded.push(BASE64[((value >> (18 - index * 6)) & 63) as usize] as char);
            } else {
                encoded.push('=');
            }
        }
    }
    encoded
}

pub fn decode_base64(text: &str) -> Option<Vec<u8>> {
    let mut bytes = Vec::with_capacity(text.len() / 4 * 3);
    let (mut buffer, mut bits) = (0u32, 0);
    for character in text.bytes() {
        if character == b'=' {
            break;
        }
        let value = BASE64.iter().position(|&candidate| candidate == character)? as u32;
        buffer = (buffer << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            bytes.push((buffer >> bits) as u8);
        }
    }
    Some(bytes)
}

macro_rules! entries {
    ($($name:ident => $body:path),* $(,)?) => {
        $(
            unsafe extern "C" fn $name(state: *mut LuaState) -> c_int {
                unsafe { guarded(state, $body) }
            }
        )*
    };
}

entries! {
    log => ui::log,
    translate => ui::translate,
    settings_switch => ui::settings_switch,
    settings_button => ui::settings_button,
    settings_choice => ui::settings_choice,
    settings_number => ui::settings_number,
    settings_text => ui::settings_text,
    settings_get => ui::settings_get,
    settings_set => ui::settings_set,
    time_after => ui::time_after,
    time_every => ui::time_every,
    time_cancel => ui::time_cancel,
    decorate => ui::decorate,
    relabel => ui::relabel,
    show_alert => ui::show_alert,
    confirm => ui::confirm,
    open_sheet => ui::open_sheet,
    update_sheet => ui::update_sheet,
    close_sheet => ui::close_sheet,
    press_node => ui::press_node,
    image_rewrite => ui::register_image_rewrite,
    text_transform => ui::register_text_transform,
    composer => ui::register_composer_decorator,
    message_background => ui::register_message_background,
    node_patch => ui::register_node_patch,
    node_insert => ui::register_node_insert,
    code_block => ui::register_code_block,
    web_auth => ui::web_auth,
    cancel_web_auth => ui::cancel_web_auth,
    app_background => ui::app_background,
    style_text => ui::style_text,
    message_action => ui::message_action,
    reaction => ui::reaction_decorator,
    pick_file => ui::pick_file,
    cancel_file_pick => ui::cancel_file_pick,
    subscribe => ui::subscribe_event,
    unsubscribe => ui::unsubscribe_event,
    gateway_dispatch => ui::dispatch_gateway_event,
    register_command => ui::register_command,
    render_image => ui::render_image,
    outgoing_transform => ui::register_outgoing_transform,
    set_message_text => ui::set_message_text,
    get_message_text => ui::get_message_text,
    send_local => ui::send_local_message,
    media_save => ui::register_media_save_handler,
    picked_media => ui::picked_media,
    picked_video_url => ui::picked_video_url,
    offer_pick => ui::offer_pick,
    network_request => ui::network_request,
    network_cancel => ui::cancel_network_request,
    intercept => http::register_intercept,
    release_held => http::release_held,
    respond_held => http::respond_held,
    discord_request => ui::discord_request,
    require => ui::require,
}

unsafe fn function(state: *mut LuaState, plugin: *mut Plugin, name: &str, body: CFunction) {
    unsafe {
        lua::lua_pushlightuserdata(state, plugin.cast());
        lua::lua_pushcclosure(state, body, 1);
        lua::set_field(state, -2, name);
    }
}

unsafe fn group(state: *mut LuaState, plugin: *mut Plugin, name: &str, entries: &[(&str, CFunction)]) {
    unsafe {
        lua::new_table(state);
        for (field, body) in entries {
            function(state, plugin, field, *body);
        }
        lua::set_field(state, -2, name);
    }
}

const PERMISSIONS: [(&str, i64); 14] = [
    ("KICK_MEMBERS", 1 << 1),
    ("BAN_MEMBERS", 1 << 2),
    ("ADMINISTRATOR", 1 << 3),
    ("MANAGE_CHANNELS", 1 << 4),
    ("MANAGE_GUILD", 1 << 5),
    ("MANAGE_MESSAGES", 1 << 13),
    ("EMBED_LINKS", 1 << 14),
    ("MUTE_MEMBERS", 1 << 22),
    ("DEAFEN_MEMBERS", 1 << 23),
    ("MOVE_MEMBERS", 1 << 24),
    ("MANAGE_ROLES", 1 << 28),
    ("MANAGE_WEBHOOKS", 1 << 29),
    ("MANAGE_THREADS", 1 << 34),
    ("MODERATE_MEMBERS", 1 << 40),
];

pub(crate) fn install_api(plugin: *mut Plugin) {
    let state = unsafe { (*plugin).state() };
    let identifier = unsafe { (*plugin).identifier().to_owned() };
    let locale = unsafe { (*plugin).package.locale.clone() };
    unsafe {
        lua::new_table(state);
        function(state, plugin, "log", log);
        lua::push_str(state, &identifier);
        lua::set_field(state, -2, "id");
        lua::push_str(state, "1");
        lua::set_field(state, -2, "api");
        group(
            state,
            plugin,
            "settings",
            &[
                ("switch", settings_switch),
                ("button", settings_button),
                ("choice", settings_choice),
                ("number", settings_number),
                ("text", settings_text),
                ("get", settings_get),
                ("set", settings_set),
            ],
        );
        lua::new_table(state);
        function(state, plugin, "t", translate);
        lua::push_str(state, &locale);
        lua::set_field(state, -2, "locale");
        lua::set_field(state, -2, "i18n");
        group(
            state,
            plugin,
            "audio",
            &[
                ("speak", api::astral_lua_audio_speak),
                ("play", api::astral_lua_audio_play),
                ("stop", api::astral_lua_audio_stop),
            ],
        );
        group(
            state,
            plugin,
            "time",
            &[
                ("now_ms", api::astral_lua_time_now_ms),
                ("after", time_after),
                ("every", time_every),
                ("cancel", time_cancel),
                ("iso", api::astral_lua_time_iso),
            ],
        );
        group(
            state,
            plugin,
            "ui",
            &[
                ("decorate", decorate),
                ("relabel", relabel),
                ("alert", show_alert),
                ("confirm", confirm),
                ("sheet", open_sheet),
                ("sheet_update", update_sheet),
                ("sheet_close", close_sheet),
                ("toast", api::astral_lua_ui_toast),
                ("copy", api::astral_lua_ui_copy),
                ("paste", api::astral_lua_ui_paste),
                ("open", api::astral_lua_ui_open),
                ("open_channel", api::astral_lua_ui_open_channel),
                ("scroll_to", api::astral_lua_ui_scroll_to),
                ("focus_composer", api::astral_lua_ui_focus_composer),
                ("dismiss", api::astral_lua_ui_dismiss),
                ("press", press_node),
                ("insert_text", api::astral_lua_ui_insert_text),
                ("refresh", api::astral_lua_ui_refresh),
                ("composer_text", api::astral_lua_ui_composer_text),
                ("image_rewrite", image_rewrite),
                ("text_transform", text_transform),
                ("notify", api::astral_lua_ui_notify),
                ("composer", composer),
                ("message_background", message_background),
                ("patch", node_patch),
                ("insert", node_insert),
                ("code_block", code_block),
                ("haptic", api::astral_lua_ui_haptic),
                ("web_auth", web_auth),
                ("cancel_web_auth", cancel_web_auth),
                ("app_background", app_background),
                ("style_text", style_text),
                ("message_action", message_action),
                ("reaction", reaction),
            ],
        );
        group(state, plugin, "files", &[("pick", pick_file), ("cancel", cancel_file_pick)]);
        group(state, plugin, "events", &[("on", subscribe), ("off", unsubscribe)]);
        group(state, plugin, "gateway", &[("dispatch", gateway_dispatch)]);
        group(state, plugin, "commands", &[("register", register_command)]);
        group(state, plugin, "image", &[("render", render_image)]);
        group(
            state,
            plugin,
            "background",
            &[("keep_alive", api::astral_lua_background_keep_alive)],
        );
        group(
            state,
            plugin,
            "diagnostics",
            &[("environment", api::astral_lua_diagnostics_environment)],
        );
        group(state, plugin, "code", &[("highlight", api::astral_lua_code_highlight)]);
        group(
            state,
            plugin,
            "json",
            &[
                ("decode", api::astral_lua_json_decode),
                ("encode", api::astral_lua_json_encode),
                ("array", api::astral_lua_json_array),
            ],
        );
        group(
            state,
            plugin,
            "messages",
            &[
                ("transform", outgoing_transform),
                ("set_text", set_message_text),
                ("get_text", get_message_text),
                ("send_local", send_local),
            ],
        );
        group(
            state,
            plugin,
            "media",
            &[
                ("on_save", media_save),
                ("picked", picked_media),
                ("picked_url", picked_video_url),
                ("offer_pick", offer_pick),
            ],
        );
        lua::new_table(state);
        function(state, plugin, "request", network_request);
        function(state, plugin, "intercept", intercept);
        function(state, plugin, "release", release_held);
        function(state, plugin, "respond", respond_held);
        lua::push_str(state, http::HOLD_REQUEST);
        lua::set_field(state, -2, "HOLD");
        function(state, plugin, "cancel", network_cancel);
        lua::set_field(state, -2, "net");
        group(
            state,
            plugin,
            "storage",
            &[
                ("get", api::astral_lua_storage_get),
                ("set", api::astral_lua_storage_set),
                ("remove", api::astral_lua_storage_remove),
                ("keys", api::astral_lua_storage_keys),
            ],
        );
        lua::new_table(state);
        for (name, body) in [
            ("guild", api::astral_lua_discord_guild as CFunction),
            ("guilds", api::astral_lua_discord_guilds),
            ("emoji", api::astral_lua_discord_emoji),
            ("me", api::astral_lua_discord_me),
            ("member", api::astral_lua_discord_member),
            ("channel_guild", api::astral_lua_discord_channel_guild),
            ("permissions", api::astral_lua_discord_permissions),
            ("message", api::astral_lua_discord_message),
            ("raw_message", api::astral_lua_discord_raw_message),
            ("request", discord_request),
            ("user", api::astral_lua_discord_user),
            ("nickname", api::astral_lua_discord_nickname),
            ("relationship", api::astral_lua_discord_relationship),
        ] {
            function(state, plugin, name, body);
        }
        lua::new_table(state);
        for (name, value) in PERMISSIONS {
            lua::lua_pushinteger(state, value);
            lua::set_field(state, -2, name);
        }
        lua::set_field(state, -2, "permission");
        lua::set_field(state, -2, "discord");
        crate::lua::enums::push_all(state);
        lua::set_field(state, -2, "enums");
        lua::lua_setglobal(state, c"astral".as_ptr());
    }
}

pub(crate) fn install_require(plugin: *mut Plugin) {
    let state = unsafe { (*plugin).state() };
    unsafe {
        lua::new_table(state);
        lua::set_field(state, lua::REGISTRYINDEX, "astral.loaded");
        lua::lua_pushlightuserdata(state, plugin.cast());
        lua::lua_pushcclosure(state, require, 1);
        lua::lua_setglobal(state, c"require".as_ptr());
    }
    let _ = host::no_reference();
}
