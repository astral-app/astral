use core::ffi::{c_char, c_int};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use serde_json::{Value, json};

use crate::settings::store::{AstralSettingsHandle, store as settings_store};
use crate::support::bridge::{AstralRustString, bytes, path};
use crate::support::files::{join_path, list_directory, read_text, resolve_inside};
use crate::support::toml_json;

const ICON_PACKS: &str = "icon-packs";
const EMOJI_PACKS: &str = "emoji-packs";
const ICON_CACHE_LIMIT: usize = 4096;

#[derive(Default)]
struct Cache {
    loaded_emoji: Option<String>,
    manifest: Option<Value>,
    coverage: HashSet<String>,
    animated_coverage: HashSet<String>,
    loaded_icons: Option<String>,
    icon_directory: String,
    icon_paths: HashMap<String, String>,
}

pub struct AstralPacksHandle {
    settings: usize,
    state: Vec<u8>,
    resources: Vec<u8>,
    cache: Mutex<Cache>,
}

fn safe_name(name: &str) -> bool {
    !name.is_empty() && name != "." && !name.contains("..") && !name.contains(['/', '\\', '\0'])
}

fn text(object: &Value, key: &str) -> String {
    object.get(key).and_then(Value::as_str).unwrap_or("").to_owned()
}

fn lines(directory: &str, name: &str) -> HashSet<String> {
    resolve_inside(directory.as_bytes(), name.as_bytes())
        .and_then(|file| read_text(path(file.as_ptr().cast(), file.len())))
        .map(|contents| {
            String::from_utf8_lossy(&contents)
                .lines()
                .map(|line| line.trim_end_matches('\r').to_owned())
                .filter(|line| !line.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn coverage_key(codepoints: &str) -> String {
    let mut key = String::new();
    for point in codepoints.split('-') {
        match point {
            "200d" => key.push('+'),
            "fe0f" => key.push('!'),
            _ => {
                if !key.is_empty() && !key.ends_with('+') {
                    key.push('-');
                }
                key.push_str(point);
            }
        }
    }
    key
}

impl AstralPacksHandle {
    fn setting_string(&self, key: &str, fallback: &str) -> String {
        unsafe { settings_store(self.settings as *const AstralSettingsHandle) }
            .and_then(|mut store| store.get_string("astral", key))
            .unwrap_or_else(|| fallback.to_owned())
    }

    fn list(&self, kind: &str) -> Vec<Value> {
        let emoji = kind == EMOJI_PACKS;
        let description = if emoji { "Discord's own emoji" } else { "Discord's own icons" };
        let mut result = vec![json!({"id": "discord", "name": "Discord", "description": description, "bundled": true})];
        let mut seen = HashSet::from(["discord".to_owned()]);
        for root in [&self.state, &self.resources] {
            let directory = join_path(root, kind.as_bytes());
            for entry in list_directory(path(directory.as_ptr().cast(), directory.len())) {
                let Some(pack) = resolve_inside(&directory, &entry) else { continue };
                let Some(manifest_path) = resolve_inside(&pack, b"manifest.toml") else {
                    continue;
                };
                let manifest = read_text(path(manifest_path.as_ptr().cast(), manifest_path.len()))
                    .and_then(|contents| String::from_utf8(contents).ok())
                    .and_then(|contents| toml_json::parse(&contents));
                let Some(mut manifest) = manifest.filter(Value::is_object) else {
                    continue;
                };
                let id = text(&manifest, "id");
                if !safe_name(&id) || text(&manifest, "name").is_empty() || seen.contains(&id) {
                    continue;
                }
                if emoji && !text(&manifest, "static").starts_with("https://") {
                    continue;
                }
                let pack_text = String::from_utf8_lossy(&pack).into_owned();
                if !emoji {
                    let icons = join_path(&pack, b"icons");
                    manifest["count"] = json!(list_directory(path(icons.as_ptr().cast(), icons.len())).len());
                }
                manifest["directory"] = json!(pack_text);
                manifest["bundled"] = json!(*root == self.resources);
                seen.insert(id);
                result.push(manifest);
            }
        }
        result
    }

    fn select(&self, kind: &str, key: &str, identifier: &str) -> bool {
        if !self.list(kind).iter().any(|pack| text(pack, "id") == identifier) {
            return false;
        }
        let Some(mut store) = (unsafe { settings_store(self.settings as *const AstralSettingsHandle) }) else {
            return false;
        };
        store.set("astral", key, json!(identifier));
        if !store.save() {
            return false;
        }
        drop(store);
        self.invalidate();
        true
    }

    fn invalidate(&self) {
        *self.cache.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Cache::default();
    }

    fn animated(&self) -> bool {
        unsafe { settings_store(self.settings as *const AstralSettingsHandle) }
            .is_none_or(|mut store| store.get_boolean("astral", "emoji-animated", true))
    }

    fn icon_path(&self, name: &str) -> String {
        if !name.starts_with("ic_") || !safe_name(name) {
            return String::new();
        }
        let active = self.setting_string("icon-pack", "discord");
        if active == "discord" {
            return String::new();
        }
        let directory = {
            let cache = self.cache.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            (cache.loaded_icons.as_deref() == Some(active.as_str())).then(|| cache.icon_directory.clone())
        };
        let directory = directory.unwrap_or_else(|| {
            let found = self
                .list(ICON_PACKS)
                .iter()
                .rev()
                .find(|pack| text(pack, "id") == active)
                .map(|pack| text(pack, "directory"))
                .unwrap_or_default();
            let mut cache = self.cache.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            cache.loaded_icons = Some(active.clone());
            cache.icon_directory = found.clone();
            cache.icon_paths.clear();
            found
        });
        let mut cache = self.cache.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(cached) = cache.icon_paths.get(name) {
            return cached.clone();
        }
        if cache.icon_paths.len() >= ICON_CACHE_LIMIT {
            cache.icon_paths.clear();
        }
        let found = [3, 2, 1]
            .into_iter()
            .filter(|_| !directory.is_empty())
            .filter_map(|scale| resolve_inside(directory.as_bytes(), format!("icons/{name}@{scale}x.png").as_bytes()))
            .map(|file| String::from_utf8_lossy(&file).into_owned())
            .find(|file| std::path::Path::new(file).exists())
            .unwrap_or_default();
        cache.icon_paths.insert(name.to_owned(), found.clone());
        found
    }

    fn emoji_url(&self, codepoints: &str) -> String {
        if codepoints.is_empty() || codepoints.len() > 96 {
            return String::new();
        }
        let lower = codepoints.to_ascii_lowercase();
        if !lower.bytes().all(|byte| byte.is_ascii_hexdigit() || byte == b'-') {
            return String::new();
        }
        let active = self.setting_string("emoji-pack", "discord");
        if active == "discord" {
            return String::new();
        }
        let animate = self.animated();
        let loaded = self
            .cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .loaded_emoji
            .as_deref()
            == Some(active.as_str());
        if !loaded {
            let pack = self.list(EMOJI_PACKS).into_iter().find(|pack| text(pack, "id") == active);
            let mut cache = self.cache.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            cache.loaded_emoji = Some(active.clone());
            cache.coverage = pack
                .as_ref()
                .map(|pack| lines(&text(pack, "directory"), "coverage.txt"))
                .unwrap_or_default();
            cache.animated_coverage = pack
                .as_ref()
                .map(|pack| lines(&text(pack, "directory"), "animated.txt"))
                .unwrap_or_default();
            cache.manifest = pack;
        }
        let cache = self.cache.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let key = coverage_key(&lower);
        let Some(manifest) = cache.manifest.as_ref().filter(|_| cache.coverage.contains(&key)) else {
            return String::new();
        };
        let animated = text(manifest, "animated");
        let pattern = if animate && cache.animated_coverage.contains(&key) && animated.starts_with("https://") {
            animated
        } else {
            text(manifest, "static")
        };
        pattern.replace("{codepoints}", &lower)
    }
}

fn handle<'a>(packs: *const AstralPacksHandle) -> Option<&'a AstralPacksHandle> {
    unsafe { packs.as_ref() }
}

fn string_of<'a>(data: *const c_char, length: usize) -> &'a str {
    std::str::from_utf8(bytes(data, length)).unwrap_or("")
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_packs_open(
    settings: *const AstralSettingsHandle,
    state: *const c_char,
    state_length: usize,
    resources: *const c_char,
    resources_length: usize,
) -> *mut AstralPacksHandle {
    Box::into_raw(Box::new(AstralPacksHandle {
        settings: settings as usize,
        state: bytes(state, state_length).to_vec(),
        resources: bytes(resources, resources_length).to_vec(),
        cache: Mutex::new(Cache::default()),
    }))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_packs_close(packs: *mut AstralPacksHandle) {
    if !packs.is_null() {
        drop(unsafe { Box::from_raw(packs) });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_packs_list(packs: *const AstralPacksHandle, emoji: c_int) -> AstralRustString {
    let list = handle(packs)
        .map(|packs| packs.list(if emoji != 0 { EMOJI_PACKS } else { ICON_PACKS }))
        .unwrap_or_default();
    AstralRustString::new(Value::Array(list).to_string().into_bytes())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_packs_select(
    packs: *const AstralPacksHandle,
    emoji: c_int,
    identifier: *const c_char,
    length: usize,
) -> c_int {
    let (kind, key) = if emoji != 0 {
        (EMOJI_PACKS, "emoji-pack")
    } else {
        (ICON_PACKS, "icon-pack")
    };
    c_int::from(handle(packs).is_some_and(|packs| packs.select(kind, key, string_of(identifier, length))))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_packs_invalidate(packs: *const AstralPacksHandle) {
    if let Some(packs) = handle(packs) {
        packs.invalidate();
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_packs_icon_path(packs: *const AstralPacksHandle, name: *const c_char, length: usize) -> AstralRustString {
    AstralRustString::new(
        handle(packs)
            .map(|packs| packs.icon_path(string_of(name, length)))
            .unwrap_or_default()
            .into_bytes(),
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_packs_emoji_url(
    packs: *const AstralPacksHandle,
    codepoints: *const c_char,
    length: usize,
) -> AstralRustString {
    AstralRustString::new(
        handle(packs)
            .map(|packs| packs.emoji_url(string_of(codepoints, length)))
            .unwrap_or_default()
            .into_bytes(),
    )
}
