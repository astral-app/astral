use core::ffi::{c_char, c_int};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::support::bridge::{AstralRustString, bytes};

const TABLE_LIMIT: usize = 1 << 20;
const MODULE_DEPTH: usize = 4;

#[derive(Clone, Copy)]
pub struct Limits {
    pub source: usize,
    pub modules: usize,
    pub name_length: usize,
    pub icon_svg: usize,
    pub authors: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            source: 256 << 10,
            modules: 1 << 20,
            name_length: 48,
            icon_svg: 16384,
            authors: 12,
        }
    }
}

#[derive(Default)]
pub struct Package {
    pub identifier: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub icon: String,
    pub icon_svg: String,
    pub main: String,
    pub directory: String,
    pub source: Vec<u8>,
    pub main_bytecode: Vec<u8>,
    pub authors: Vec<(String, String)>,
    pub permissions: Vec<String>,
    pub enabled_by_default: bool,
    pub modules: BTreeMap<String, (Vec<u8>, Vec<u8>)>,
    pub strings: BTreeMap<String, String>,
    pub fallback_strings: BTreeMap<String, String>,
    pub locale: String,
}

fn language_of(locale: &str) -> &str {
    locale.split('-').next().unwrap_or(locale)
}

pub fn best_locale(available: &[String], preferred: &[String]) -> Option<String> {
    let mut wanted: Vec<String> = Vec::new();
    for language in preferred {
        wanted.push(language.clone());
        wanted.push(language_of(language).to_owned());
    }
    wanted.extend(["en-US", "en-GB", "en"].map(str::to_owned));
    for candidate in &wanted {
        if let Some(found) = available.iter().find(|locale| locale.eq_ignore_ascii_case(candidate)) {
            return Some(found.clone());
        }
        if let Some(found) = available.iter().find(|locale| language_of(locale).eq_ignore_ascii_case(candidate)) {
            return Some(found.clone());
        }
    }
    None
}

pub fn fill_placeholders(text: &str, values: &[(String, String)]) -> String {
    let mut text = text.to_owned();
    for (name, value) in values {
        let token = format!("{{{name}}}");
        let mut at = 0;
        while let Some(found) = text[at..].find(&token) {
            let start = at + found;
            text.replace_range(start..start + token.len(), value);
            at = start + value.len();
        }
    }
    text
}

pub fn valid_plugin_identifier(identifier: &str) -> bool {
    (3..=80).contains(&identifier.len())
        && !identifier.contains("..")
        && identifier
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'.' || byte == b'-')
}

fn read_limited(path: &Path, limit: usize) -> Option<Vec<u8>> {
    let size = fs::metadata(path).ok()?.len();
    if size > limit as u64 {
        return None;
    }
    fs::read(path).ok()
}

fn read_table(path: &Path) -> Option<toml::Table> {
    let text = String::from_utf8(read_limited(path, TABLE_LIMIT)?).ok()?;
    text.parse::<toml::Table>().ok()
}

fn string_at(table: &toml::Table, key: &str, fallback: &str) -> String {
    table.get(key).and_then(toml::Value::as_str).unwrap_or(fallback).to_owned()
}

fn truncate_codepoints(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

fn load_strings(path: &Path) -> BTreeMap<String, String> {
    read_table(path)
        .map(|table| {
            table
                .into_iter()
                .filter_map(|(key, value)| value.as_str().map(|text| (key, text.to_owned())))
                .collect()
        })
        .unwrap_or_default()
}

fn load_translations(package: &mut Package, preferred: &[String]) {
    let folder = Path::new(&package.directory).join("i18n");
    let available: Vec<String> = fs::read_dir(&folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "toml"))
        .filter_map(|path| path.file_stem().map(|stem| stem.to_string_lossy().into_owned()))
        .collect();
    let english: Vec<String> = available.iter().filter(|locale| locale.starts_with("en")).cloned().collect();
    if let Some(locale) = best_locale(&available, preferred) {
        package.strings = load_strings(&folder.join(format!("{locale}.toml")));
        package.locale = locale;
    }
    let fallback = if english.iter().any(|locale| locale == "en-US") {
        Some("en-US".to_owned())
    } else {
        best_locale(&english, preferred)
    };
    if let Some(fallback) = fallback {
        package.fallback_strings = load_strings(&folder.join(format!("{fallback}.toml")));
    }
}

fn module_name(relative: &Path) -> String {
    relative
        .with_extension("")
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(".")
}

fn walk(directory: &Path, relative: &Path, depth: usize, files: &mut Vec<PathBuf>) -> bool {
    let Ok(entries) = fs::read_dir(directory) else { return false };
    for entry in entries {
        let Ok(entry) = entry else { return false };
        let name = PathBuf::from(entry.file_name());
        let child = relative.join(&name);
        if depth == 0 && name == Path::new("tests") {
            continue;
        }
        let Ok(kind) = entry.file_type() else { return false };
        if kind.is_dir() {
            if depth < MODULE_DEPTH && !walk(&entry.path(), &child, depth + 1, files) {
                return false;
            }
        } else if fs::metadata(entry.path()).is_ok_and(|metadata| metadata.is_file()) && depth <= MODULE_DEPTH {
            files.push(child);
        }
    }
    true
}

fn load_modules(package: &mut Package, limit: usize) -> bool {
    let root = PathBuf::from(&package.directory);
    let mut files = Vec::new();
    if !walk(&root, Path::new(""), 0, &mut files) {
        return false;
    }
    let mut total = 0;
    for relative in files {
        if relative.extension().is_none_or(|extension| extension != "lua") || relative == Path::new(&package.main) {
            continue;
        }
        let path = root.join(&relative);
        let Some(source) = read_limited(&path, limit) else { return false };
        total += source.len();
        if total > limit {
            return false;
        }
        let bytecode = read_limited(&path.with_extension("luac"), limit).unwrap_or_default();
        package.modules.insert(module_name(&relative), (source, bytecode));
    }
    true
}

fn authors_of(manifest: &toml::Table, limit: usize) -> Vec<(String, String)> {
    let Some(list) = manifest.get("authors").and_then(toml::Value::as_array) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|author| match author {
            toml::Value::String(name) => Some((name.clone(), String::new())),
            toml::Value::Table(table) => table
                .get("name")
                .and_then(toml::Value::as_str)
                .map(|name| (name.to_owned(), string_at(table, "id", ""))),
            _ => None,
        })
        .take(limit)
        .collect()
}

pub fn load(directory: &str, preferred: &[String], limits: Limits) -> Option<Package> {
    let root = Path::new(directory);
    let manifest = read_table(&root.join("manifest.toml"))?;
    let mut package = Package {
        identifier: string_at(&manifest, "id", ""),
        name: string_at(&manifest, "name", ""),
        main: string_at(&manifest, "main", "main.lua"),
        ..Package::default()
    };
    if !valid_plugin_identifier(&package.identifier) || package.name.is_empty() || package.main.contains('/') {
        return None;
    }
    package.source = read_limited(&root.join(&package.main), limits.source)?;
    package.main_bytecode = read_limited(&root.join(&package.main).with_extension("luac"), limits.source * 2).unwrap_or_default();
    package.directory = directory.to_owned();
    package.name = truncate_codepoints(&package.name, limits.name_length);
    package.version = string_at(&manifest, "version", "");
    package.description = string_at(&manifest, "description", "");
    package.icon = string_at(&manifest, "icon", "");
    package.enabled_by_default = manifest.get("enabled_by_default").and_then(toml::Value::as_bool).unwrap_or(false);
    let icon_svg = string_at(&manifest, "iconSvg", "");
    if icon_svg.len() < limits.icon_svg {
        package.icon_svg = icon_svg;
    }
    package.authors = authors_of(&manifest, limits.authors);
    let mut permissions: Vec<String> = manifest
        .get("permissions")
        .and_then(toml::Value::as_array)
        .map(|list| list.iter().filter_map(|value| value.as_str().map(str::to_owned)).collect())
        .unwrap_or_default();
    permissions.sort();
    permissions.dedup();
    package.permissions = permissions;
    load_translations(&mut package, preferred);
    if !load_modules(&mut package, limits.modules) {
        return None;
    }
    Some(package)
}

fn header(package: &Package) -> Value {
    json!({
        "identifier": package.identifier,
        "name": package.name,
        "version": package.version,
        "description": package.description,
        "icon": package.icon,
        "iconSvg": package.icon_svg,
        "main": package.main,
        "directory": package.directory,
        "authors": package.authors,
        "permissions": package.permissions,
        "enabledByDefault": package.enabled_by_default,
        "locale": package.locale,
        "strings": package.strings,
        "fallbackStrings": package.fallback_strings,
    })
}

fn list_of(data: *const c_char, length: usize) -> Vec<String> {
    bytes(data, length)
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .map(|entry| String::from_utf8_lossy(entry).into_owned())
        .collect()
}

fn none() -> AstralRustString {
    AstralRustString {
        bytes: core::ptr::null_mut(),
        length: 0,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_plugin_package_load(
    directory: *const c_char,
    directory_length: usize,
    preferred: *const c_char,
    preferred_length: usize,
    source_limit: usize,
    modules_limit: usize,
) -> AstralRustString {
    let directory = String::from_utf8_lossy(bytes(directory, directory_length)).into_owned();
    let limits = Limits {
        source: source_limit,
        modules: modules_limit,
        ..Limits::default()
    };
    let Some(package) = load(&directory, &list_of(preferred, preferred_length), limits) else {
        return none();
    };
    let mut frames = vec![
        header(&package).to_string().into_bytes(),
        package.source.clone(),
        package.main_bytecode.clone(),
    ];
    for (name, (source, bytecode)) in &package.modules {
        frames.push(name.clone().into_bytes());
        frames.push(source.clone());
        frames.push(bytecode.clone());
    }
    AstralRustString::frames(&frames)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_lua_best_locale(
    available: *const c_char,
    available_length: usize,
    preferred: *const c_char,
    preferred_length: usize,
) -> AstralRustString {
    best_locale(&list_of(available, available_length), &list_of(preferred, preferred_length))
        .map_or_else(none, |locale| AstralRustString::new(locale.into_bytes()))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_lua_fill_placeholders(
    text: *const c_char,
    text_length: usize,
    values: *const c_char,
    values_length: usize,
) -> AstralRustString {
    let pairs: Vec<String> = list_of_frames(bytes(values, values_length));
    let values: Vec<(String, String)> = pairs
        .as_chunks::<2>()
        .0
        .iter()
        .map(|[name, value]| (name.clone(), value.clone()))
        .collect();
    let text = String::from_utf8_lossy(bytes(text, text_length));
    AstralRustString::new(fill_placeholders(&text, &values).into_bytes())
}

fn list_of_frames(data: &[u8]) -> Vec<String> {
    let mut entries = Vec::new();
    let mut at = 0;
    while at + 4 <= data.len() {
        let length = u32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]]) as usize;
        at += 4;
        if length > data.len() - at {
            break;
        }
        entries.push(String::from_utf8_lossy(&data[at..at + length]).into_owned());
        at += length;
    }
    entries
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_lua_valid_plugin_identifier(identifier: *const c_char, length: usize) -> c_int {
    c_int::from(valid_plugin_identifier(&String::from_utf8_lossy(bytes(identifier, length))))
}
