use core::ffi::{c_char, c_int, c_void};

use serde_json::Value;

use super::ffi::{collect, raw, take, take_json, take_optional};
use super::host;
use super::state::{App, app, signing_key, wire};
use crate::lua::host as lua_host;
use crate::plugins::marketplace::{KIND_EMOJI_PACK, KIND_ICON_PACK};
use crate::support::bridge::AstralRustString;

const DEFAULT_THEME: &str = "discord";
const MARKETPLACE_THEME: c_int = 1;
const PLUGIN_ICON_PREFIX: &str = "astral_plugin_";
const PLUGIN_ICON_LIMIT: usize = 16_384;
const SURFACE_COUNT: usize = 3;

pub fn active(current: &App) -> String {
    current.text("astral", "theme", DEFAULT_THEME)
}

pub fn list(current: &App) -> Value {
    let (state, state_length) = raw(&current.state);
    let (resources, resources_length) = raw(&current.resources);
    take_json(unsafe { crate::plugins::themes::astral_themes_list(state, state_length, resources, resources_length) })
}

fn colors(style: &Value, key: &str) -> Vec<u32> {
    style
        .get(key)
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_u64)
                .filter_map(|value| u32::try_from(value).ok())
                .collect()
        })
        .unwrap_or_default()
}

fn number(style: &Value, key: &str) -> f64 {
    style.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

fn apply_style(style: &Value) {
    use crate::wire::theme;
    let from = colors(style, "from");
    let to = colors(style, "to");
    let gradient = colors(style, "gradient");
    let count = from.len().min(to.len());
    unsafe {
        theme::astral_wire_set_palette(from.as_ptr(), to.as_ptr(), count);
        theme::astral_wire_set_background_gradient(gradient.as_ptr(), gradient.len());
    }
    theme::astral_wire_set_scales(number(style, "radius"), number(style, "font"));
    let image = style.get("image").and_then(Value::as_str).unwrap_or_default();
    let opacity = number(style, "imageOpacity");
    unsafe { crate::wire::backdrop::astral_wire_set_background_image(image.as_ptr(), image.len(), opacity) };
    let tint = style
        .get("tint")
        .and_then(Value::as_u64)
        .and_then(|tint| u32::try_from(tint).ok())
        .unwrap_or(0);
    theme::astral_wire_set_tint(tint, number(style, "saturation"), number(style, "neutral"));
    let surfaces = style.get("surfaces").and_then(Value::as_array).cloned().unwrap_or_default();
    for (index, surface) in surfaces.iter().take(SURFACE_COUNT).enumerate() {
        let color = surface
            .get("color")
            .and_then(Value::as_u64)
            .and_then(|color| u32::try_from(color).ok())
            .unwrap_or(0);
        if color != 0 {
            theme::astral_wire_set_surface_tint(c_int::try_from(index).unwrap_or(0), color, number(surface, "saturation"));
        }
    }
}

pub fn apply(current: &App, identifier: &str) {
    let (state, state_length) = raw(&current.state);
    let (resources, resources_length) = raw(&current.resources);
    let (pointer, length) = raw(identifier);
    let style = take_json(unsafe {
        crate::plugins::themes::astral_themes_select(
            current.settings_handle(),
            state,
            state_length,
            resources,
            resources_length,
            pointer,
            length,
        )
    });
    let bytes = {
        let _held = wire();
        if style.is_object() {
            apply_style(&style);
        }
        let mut bytes = Vec::new();
        collect(&mut bytes, |buffer| unsafe { crate::wire::theme::astral_wire_recolor(buffer) });
        collect(&mut bytes, |buffer| unsafe {
            crate::wire::backdrop::astral_wire_refresh_backdrop(buffer)
        });
        bytes
    };
    host::deliver(&bytes);
}

pub fn import_unbound(current: &App, source: &str, fallback: &str) -> Option<String> {
    let (state, state_length) = raw(&current.state);
    let (resources, resources_length) = raw(&current.resources);
    let (source_pointer, source_length) = raw(source);
    let (fallback_pointer, fallback_length) = raw(fallback);
    take_optional(unsafe {
        crate::plugins::themes::astral_themes_import_unbound(
            current.settings_handle(),
            state,
            state_length,
            resources,
            resources_length,
            source_pointer,
            source_length,
            fallback_pointer,
            fallback_length,
        )
    })
}

pub fn fetch(url: &str) -> (c_int, Vec<u8>) {
    crate::network::http::fetch(url)
}

unsafe extern "C" fn marketplace_fetch(_: *mut c_void, url: *const c_char, length: usize, body: *mut AstralRustString) -> c_int {
    let url = String::from_utf8_lossy(crate::support::bridge::bytes(url, length)).into_owned();
    let (status, bytes) = fetch(&url);
    if !body.is_null() {
        unsafe { *body = AstralRustString::new(bytes) };
    }
    status
}

unsafe extern "C" fn marketplace_installed(_: *mut c_void, kind: c_int, identifier: *const c_char, length: usize) {
    let identifier = String::from_utf8_lossy(crate::support::bridge::bytes(identifier, length)).into_owned();
    let Some(current) = app() else { return };
    if kind == KIND_ICON_PACK || kind == KIND_EMOJI_PACK {
        current.invalidate_packs();
        return;
    }
    if kind == MARKETPLACE_THEME {
        if active(current) == identifier {
            apply(current, &identifier);
        }
        return;
    }
    lua_host::remove(&identifier);
    lua_host::rescan();
}

pub fn configure_marketplace(current: &App) {
    let (state, state_length) = raw(&current.state);
    let (resources, resources_length) = raw(&current.resources);
    let (key, key_length) = raw(signing_key());
    unsafe {
        crate::plugins::marketplace::astral_marketplace_configure(
            current.settings_handle(),
            state,
            state_length,
            resources,
            resources_length,
            key,
            key_length,
            Some(marketplace_fetch),
            core::ptr::null_mut(),
            Some(marketplace_installed),
            core::ptr::null_mut(),
        );
    }
}

pub fn export_backup(current: &App) -> String {
    let (version, version_length) = raw(&current.version);
    let plugins = host::json_text(&lua_host::plugins_json());
    let themes = host::json_text(&list(current));
    let (plugins_pointer, plugins_length) = raw(&plugins);
    let (themes_pointer, themes_length) = raw(&themes);
    take(unsafe {
        crate::plugins::backup::astral_backup_export(
            current.settings_handle(),
            version,
            version_length,
            plugins_pointer,
            plugins_length,
            themes_pointer,
            themes_length,
        )
    })
}

pub fn apply_backup(current: &App, contents: &str) -> Option<String> {
    let plugins = lua_host::plugins_json();
    let plugin_text = host::json_text(&plugins);
    let themes = host::json_text(&list(current));
    let (contents_pointer, contents_length) = raw(contents);
    let (plugins_pointer, plugins_length) = raw(&plugin_text);
    let (themes_pointer, themes_length) = raw(&themes);
    let mut result = AstralRustString::new(Vec::new());
    let imported = unsafe {
        crate::plugins::backup::astral_backup_import(
            current.settings_handle(),
            contents_pointer,
            contents_length,
            plugins_pointer,
            plugins_length,
            themes_pointer,
            themes_length,
            &raw mut result,
        )
    };
    let message = take(result);
    if imported == 0 {
        return Some(message);
    }
    let installs: Value = serde_json::from_str(&message).unwrap_or(Value::Null);
    for install in installs.as_array().into_iter().flatten() {
        let kind = PackageKind::parse(install.get("kind").and_then(Value::as_str).unwrap_or_default());
        marketplace_install(install.get("package").unwrap_or(&Value::Null), kind, refresh_navigator());
    }
    for plugin in plugins.as_array().into_iter().flatten() {
        let Some(identifier) = plugin.get("id").and_then(Value::as_str).filter(|identifier| !identifier.is_empty()) else {
            continue;
        };
        let fallback = plugin.get("enabled").and_then(Value::as_bool).unwrap_or(false);
        lua_host::set_enabled(identifier, current.flag("plugins", &format!("{identifier}.enabled"), fallback));
    }
    super::soft_restart_without_reload(current);
    None
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PackageKind {
    Plugin,
    Theme,
    IconPack,
    EmojiPack,
}

impl PackageKind {
    pub fn parse(kind: &str) -> PackageKind {
        match kind {
            "theme" => PackageKind::Theme,
            "icon-pack" => PackageKind::IconPack,
            "emoji-pack" => PackageKind::EmojiPack,
            _ => PackageKind::Plugin,
        }
    }

    const fn code(self) -> c_int {
        match self {
            PackageKind::Plugin => 0,
            PackageKind::Theme => MARKETPLACE_THEME,
            PackageKind::IconPack => KIND_ICON_PACK,
            PackageKind::EmojiPack => KIND_EMOJI_PACK,
        }
    }
}

pub type Changed = std::sync::Arc<dyn Fn() + Send + Sync>;

pub fn refresh_navigator() -> Changed {
    std::sync::Arc::new(super::refresh_settings)
}

unsafe extern "C" fn notify_changed(context: *mut c_void) {
    if let Some(changed) = unsafe { context.cast::<Changed>().as_ref() } {
        changed();
    }
}

unsafe extern "C" fn release_changed(context: *mut c_void) {
    if !context.is_null() {
        drop(unsafe { Box::from_raw(context.cast::<Changed>()) });
    }
}

fn changed_context(changed: Changed) -> *mut c_void {
    Box::into_raw(Box::new(changed)).cast()
}

pub fn marketplace_install(package: &Value, kind: PackageKind, changed: Changed) {
    let json = host::json_text(package);
    let (pointer, length) = raw(&json);
    unsafe {
        crate::plugins::marketplace::astral_marketplace_install(
            pointer,
            length,
            kind.code(),
            Some(notify_changed),
            changed_context(changed),
            Some(release_changed),
        );
    }
}

pub fn marketplace_refresh(changed: Changed) {
    unsafe {
        crate::plugins::marketplace::astral_marketplace_refresh(Some(notify_changed), changed_context(changed), Some(release_changed))
    };
}

pub fn plugin_icon_path(name: &str) -> String {
    let Some(identifier) = name.strip_prefix(PLUGIN_ICON_PREFIX) else {
        return String::new();
    };
    if !crate::plugins::package::valid_plugin_identifier(identifier) {
        return String::new();
    }
    let Some(current) = app() else { return String::new() };
    let plugins = lua_host::plugins_json();
    let Some(plugin) = plugins
        .as_array()
        .into_iter()
        .flatten()
        .find(|plugin| plugin.get("id").and_then(Value::as_str) == Some(identifier))
    else {
        return String::new();
    };
    let svg = plugin.get("iconSvg").and_then(Value::as_str).unwrap_or_default();
    if svg.is_empty() || svg.len() > PLUGIN_ICON_LIMIT {
        return String::new();
    }
    let directory = format!("{}/plugin-icons", current.state);
    let path = format!("{directory}/{identifier}.svg");
    let cached = std::fs::read_to_string(&path).ok();
    if cached.as_deref() != Some(svg)
        && (std::fs::create_dir_all(&directory).is_err()
            || !crate::support::files::write_atomically(std::path::Path::new(&path), svg.as_bytes()))
    {
        return String::new();
    }
    path
}

pub fn plugin_icon_uri(plugin: &Value) -> Option<String> {
    let identifier = plugin.get("id").and_then(Value::as_str).unwrap_or_default();
    let svg = plugin.get("iconSvg").and_then(Value::as_str).unwrap_or_default();
    (!svg.is_empty() && crate::plugins::package::valid_plugin_identifier(identifier))
        .then(|| format!("res://{PLUGIN_ICON_PREFIX}{identifier}"))
}

const SNOWFLAKE_LENGTHS: core::ops::RangeInclusive<usize> = 15..=20;
const DEFAULT_AVATARS: usize = 6;

pub fn author_avatar(user_id: &str, index: usize) -> String {
    let fallback = format!("https://cdn.discordapp.com/embed/avatars/{}.png", index % DEFAULT_AVATARS);
    if !SNOWFLAKE_LENGTHS.contains(&user_id.len()) || !user_id.bytes().all(|byte| byte.is_ascii_digit()) {
        return fallback;
    }
    let Some(avatar) = crate::discord::cache::user(user_id)
        .and_then(|user| user.get("avatar").and_then(Value::as_str).map(str::to_owned))
        .filter(|avatar| !avatar.is_empty())
    else {
        return fallback;
    };
    format!("https://cdn.discordapp.com/avatars/{user_id}/{avatar}.png?size=64")
}
