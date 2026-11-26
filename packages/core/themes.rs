use core::ffi::c_char;
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use serde_json::{Map, Value, json};

use crate::bridge::{AstralRustString, bytes, optional, path};
use crate::files::{join_path, list_directory, read_text, resolve_inside, write_atomically};
use crate::settings::{AstralSettingsHandle, store as settings_store};
use crate::toml_json;
use crate::wire::theme::astral_wire_surface_of;

const RGB_HEX_LENGTH: usize = 6;
const ARGB_HEX_LENGTH: usize = 8;

const DISCORD_BLURPLE: u32 = 0xFF58_65F2;
const SURFACE_BACKGROUNDS: usize = 0;
const SURFACE_TEXT: usize = 1;
const SURFACE_ACCENTS: usize = 2;
const SURFACE_NAMES: [&str; 3] = ["backgrounds", "text", "accents"];

fn text(object: &Value, key: &str) -> String {
    object.get(key).and_then(Value::as_str).unwrap_or("").to_owned()
}

fn object(parent: &Value, key: &str) -> Value {
    parent
        .get(key)
        .filter(|value| value.is_object())
        .cloned()
        .unwrap_or_else(|| Value::Object(Map::new()))
}

fn number(parent: &Value, key: &str) -> f64 {
    parent.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

fn entries(value: &Value) -> impl Iterator<Item = (&String, &Value)> {
    value.as_object().into_iter().flatten()
}

fn color(hex: &str) -> u32 {
    let hex = hex.strip_prefix('#').unwrap_or(hex);
    let full = if hex.len() == RGB_HEX_LENGTH {
        format!("FF{hex}")
    } else {
        hex.to_owned()
    };
    if full.len() != ARGB_HEX_LENGTH || !full.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return 0;
    }
    u32::from_str_radix(&full, 16).unwrap_or(0)
}

fn argb(hex: &str) -> String {
    let hex = hex.strip_prefix('#').unwrap_or(hex);
    match hex.len() {
        RGB_HEX_LENGTH => format!("FF{hex}"),
        ARGB_HEX_LENGTH if hex.is_ascii() => format!("{}{}", &hex[6..], &hex[..6]),
        _ => String::new(),
    }
}

fn semantic(entry: &Value, theme: &str, raw: &Value) -> u32 {
    let Some([name, opacity]) = entry.get(theme).and_then(Value::as_array).map(Vec::as_slice) else {
        return 0;
    };
    let (Some(name), Some(opacity)) = (name.as_str(), opacity.as_f64()) else {
        return 0;
    };
    let base = color(&argb(&text(raw, name)));
    let alpha = (f64::from(base >> 24) * opacity.clamp(0.0, 1.0)).round() as u32;
    (base & 0xFF_FFFF) | (alpha << 24)
}

fn slug(name: &str) -> String {
    let mut result = String::new();
    for character in name.bytes().map(|byte| byte.to_ascii_lowercase()) {
        if character.is_ascii_alphanumeric() {
            result.push(character as char);
        } else if !result.is_empty() && !result.ends_with('-') {
            result.push('-');
        }
    }
    if result.ends_with('-') {
        result.pop();
    }
    if result.is_empty() { "imported".to_owned() } else { result }
}

struct Themes<'a> {
    state: &'a [u8],
    resources: &'a [u8],
}

impl Themes<'_> {
    fn path(&self, joined: &[u8]) -> std::path::PathBuf {
        path(joined.as_ptr().cast(), joined.len()).to_path_buf()
    }

    fn directory(&self) -> Vec<u8> {
        join_path(self.state, b"themes")
    }

    fn list(&self) -> Vec<Value> {
        let mut result =
            vec![json!({"id": "discord", "name": "Discord", "description": "Discord's own colours", "colors": {}, "bundled": true})];
        let mut seen: HashSet<String> = HashSet::from(["discord".to_owned()]);
        for root in [self.state, self.resources] {
            let folder = join_path(root, b"themes");
            for name in list_directory(&self.path(&folder)) {
                if !name.ends_with(b".toml") || name.len() <= 5 {
                    continue;
                }
                let Some(file) = resolve_inside(&folder, &name) else { continue };
                let Some(contents) = read_text(&self.path(&file)) else { continue };
                let Some(mut theme) = std::str::from_utf8(&contents).ok().and_then(toml_json::parse) else {
                    continue;
                };
                let id = text(&theme, "id");
                if id.is_empty()
                    || text(&theme, "name").is_empty()
                    || !theme.get("colors").is_some_and(Value::is_object)
                    || !seen.insert(id)
                {
                    continue;
                }
                theme["path"] = Value::String(String::from_utf8_lossy(&file).into_owned());
                theme["bundled"] = Value::Bool(root == self.resources);
                result.push(theme);
            }
        }
        result
    }

    fn palette(&self, name: &str) -> Value {
        let file = join_path(self.resources, format!("palettes/discord-{name}.toml").as_bytes());
        read_text(&self.path(&file))
            .and_then(|source| String::from_utf8(source).ok())
            .and_then(|source| toml_json::parse(&source))
            .map_or_else(|| Value::Object(Map::new()), |parsed| object(&parsed, "colors"))
    }

    fn style(&self, theme: &Value) -> Value {
        let mut mapped: BTreeMap<u32, u32> = BTreeMap::new();
        for (key, value) in entries(&object(theme, "colors")) {
            let (from, to) = (color(key), value.as_str().map_or(0, color));
            if from != 0 && to != 0 {
                mapped.insert(from, to);
            }
        }
        let raw = self.palette("raw");
        let tokens = self.palette("semantic");
        let original = mapped.clone();
        for (_, entry) in entries(&tokens) {
            let Some(to) = original.get(&semantic(entry, "dark", &raw)) else {
                continue;
            };
            for base in ["ash", "onyx", "light"] {
                let source = semantic(entry, base, &raw);
                if source != 0 {
                    mapped.entry(source).or_insert(*to);
                }
            }
        }
        let background = object(theme, "background");
        let gradient: Vec<u32> = background
            .get("gradient")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|stop| stop.as_str().map_or(0, color))
            .filter(|stop| *stop != 0)
            .collect();
        let tint = object(theme, "tint");
        let surfaces: Vec<Value> = SURFACE_NAMES
            .iter()
            .map(|name| {
                let surface = object(&tint, name);
                json!({"color": color(&text(&surface, "color")), "saturation": number(&surface, "saturation")})
            })
            .collect();
        json!({
            "from": mapped.keys().collect::<Vec<_>>(),
            "to": mapped.values().collect::<Vec<_>>(),
            "gradient": gradient,
            "radius": number(&object(theme, "radius"), "scale"),
            "font": number(&object(theme, "font"), "scale"),
            "tint": color(&text(&tint, "color")),
            "saturation": number(&tint, "saturation"),
            "neutral": number(&tint, "neutral"),
            "surfaces": surfaces,
        })
    }

    unsafe fn import_unbound(&self, settings: *const AstralSettingsHandle, source: &[u8], fallback: &str) -> Option<&'static str> {
        let Ok(parsed) = serde_json::from_slice::<Value>(source) else {
            return Some("not an Unbound theme");
        };
        if !parsed.is_object() {
            return Some("not an Unbound theme");
        }
        let pick = |first: &str, second: &str| {
            let value = object(&parsed, first);
            if value.as_object().is_some_and(Map::is_empty) {
                object(&parsed, second)
            } else {
                value
            }
        };
        let raw = pick("raw", "rawColors");
        let tokens = pick("semantic", "semanticColors");
        if raw.as_object().is_some_and(Map::is_empty) && tokens.as_object().is_some_and(Map::is_empty) {
            return Some("the theme has no raw or semantic colours to convert");
        }
        let base = self.palette("raw");
        let semantics = self.palette("semantic");
        let mut colors: BTreeMap<u32, u32> = BTreeMap::new();
        for (name, replacement) in entries(&raw) {
            let from = color(&argb(&text(&base, name)));
            let to = replacement.as_str().map_or(0, |value| color(&argb(value)));
            if from != 0 && to != 0 && from != to {
                colors.insert(from, to);
            }
        }
        for (name, replacement) in entries(&tokens) {
            let from = semantic(&object(&semantics, name), "dark", &base);
            let value = match replacement {
                Value::Array(values) => values.first().cloned().unwrap_or(Value::Null),
                other => other.clone(),
            };
            let to = value.as_str().map_or(0, |value| color(&argb(value)));
            if from != 0 && to != 0 && from != to {
                colors.insert(from, to);
            }
        }
        if colors.is_empty() {
            return Some("none of the theme's colours match Discord's palette");
        }
        let mut name = text(&parsed, "name");
        if name.is_empty() {
            name = fallback.to_owned();
        }
        let identifier = format!("unbound-{}", slug(&name));
        let mut out = format!(
            "id = {}\nname = {}\nversion = '0.0.1'\ndescription = 'Imported from an Unbound theme'\n\n[colors]\n",
            Value::String(identifier.clone()),
            Value::String(name)
        );
        let mut tints = [0u32; 3];
        let mut chroma = [0f64; 3];
        for (from, to) in &colors {
            out.push_str(&format!("{from:08X} = '{to:08X}'\n"));
            let rgb = [(to >> 16) & 255, (to >> 8) & 255, to & 255];
            let mut score = f64::from(rgb.iter().max().unwrap_or(&0) - rgb.iter().min().unwrap_or(&0)) / 255.0;
            if *from == DISCORD_BLURPLE {
                score += 1.0;
            }
            let surface = (astral_wire_surface_of(*from).max(0) as usize).min(2);
            if score > chroma[surface] {
                tints[surface] = *to;
                chroma[surface] = score;
            }
        }
        let tint = [tints[SURFACE_ACCENTS], tints[SURFACE_BACKGROUNDS], tints[SURFACE_TEXT]]
            .into_iter()
            .find(|tint| *tint != 0)
            .unwrap_or(0);
        if tint != 0 {
            out.push_str(&format!(
                "\n[tint]\ncolor = '#{:06X}'\nsaturation = 0.4\nneutral = 0.3\n",
                tint & 0xFF_FFFF
            ));
        }
        if tints[SURFACE_BACKGROUNDS] != 0 && chroma[SURFACE_BACKGROUNDS] > 0.03 {
            out.push_str(&format!(
                "\n[tint.backgrounds]\ncolor = '#{:06X}'\nsaturation = 0.35\n",
                tints[SURFACE_BACKGROUNDS] & 0xFF_FFFF
            ));
        }
        if tints[SURFACE_TEXT] != 0 && chroma[SURFACE_TEXT] > 0.03 {
            out.push_str(&format!(
                "\n[tint.text]\ncolor = '#{:06X}'\nsaturation = 0.4\n",
                tints[SURFACE_TEXT] & 0xFF_FFFF
            ));
        }
        let directory = self.directory();
        let target = join_path(&directory, format!("{identifier}.toml").as_bytes());
        let _ = std::fs::create_dir_all(self.path(&directory));
        if !Path::new(&self.path(&directory)).is_dir() || !write_atomically(&self.path(&target), out.as_bytes()) {
            return Some("could not save the theme");
        }
        let Some(mut store) = (unsafe { settings_store(settings) }) else {
            return Some("could not select the imported theme");
        };
        store.set("astral", "theme", Value::String(identifier));
        if !store.save() {
            return Some("could not select the imported theme");
        }
        None
    }
}

fn themes<'a>(state: *const c_char, state_length: usize, resources: *const c_char, resources_length: usize) -> Themes<'a> {
    Themes {
        state: bytes(state, state_length),
        resources: bytes(resources, resources_length),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_themes_list(
    state: *const c_char,
    state_length: usize,
    resources: *const c_char,
    resources_length: usize,
) -> AstralRustString {
    AstralRustString::new(
        Value::Array(themes(state, state_length, resources, resources_length).list())
            .to_string()
            .into_bytes(),
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_themes_select(
    settings: *const AstralSettingsHandle,
    state: *const c_char,
    state_length: usize,
    resources: *const c_char,
    resources_length: usize,
    identifier: *const c_char,
    identifier_length: usize,
) -> AstralRustString {
    let themes = themes(state, state_length, resources, resources_length);
    let wanted = std::str::from_utf8(bytes(identifier, identifier_length)).unwrap_or("");
    let list = themes.list();
    let selected = list.iter().rev().find(|theme| text(theme, "id") == wanted).unwrap_or(&list[0]);
    let style = themes.style(selected);
    if let Some(mut store) = unsafe { settings_store(settings) }
        && store.set("astral", "theme", Value::String(text(selected, "id")))
    {
        store.save();
    }
    AstralRustString::new(style.to_string().into_bytes())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_themes_import_unbound(
    settings: *const AstralSettingsHandle,
    state: *const c_char,
    state_length: usize,
    resources: *const c_char,
    resources_length: usize,
    source: *const c_char,
    source_length: usize,
    fallback: *const c_char,
    fallback_length: usize,
) -> AstralRustString {
    let themes = themes(state, state_length, resources, resources_length);
    let fallback = std::str::from_utf8(bytes(fallback, fallback_length)).unwrap_or("Imported");
    let error = unsafe { themes.import_unbound(settings, bytes(source, source_length), fallback) };
    optional(error.map(|error| error.as_bytes().to_vec()))
}
