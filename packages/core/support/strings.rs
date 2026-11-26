use core::ffi::c_char;
use std::collections::HashMap;
use std::sync::Mutex;

use crate::support::bridge::{AstralRustString, bytes, path};
use crate::support::files::{join_path, read_text};

const STRINGS_FILE: &[u8] = b"i18n/astral.json";

static STRINGS: Mutex<Option<HashMap<Vec<u8>, Vec<u8>>>> = Mutex::new(None);

pub fn set_strings(json: &[u8]) {
    let strings = match serde_json::from_slice::<serde_json::Value>(json) {
        Ok(serde_json::Value::Object(entries)) => entries
            .into_iter()
            .filter_map(|(key, value)| match value {
                serde_json::Value::String(text) if !text.is_empty() => Some((key.into_bytes(), text.into_bytes())),
                _ => None,
            })
            .collect(),
        _ => HashMap::new(),
    };
    *STRINGS.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(strings);
}

fn substitute(mut message: Vec<u8>, values: &[(Vec<u8>, Vec<u8>)]) -> Vec<u8> {
    for (name, value) in values {
        let mut placeholder = vec![b'{'];
        placeholder.extend_from_slice(name);
        placeholder.push(b'}');
        let mut at = 0;
        while let Some(found) = message[at..]
            .windows(placeholder.len())
            .position(|window| window == placeholder.as_slice())
        {
            let start = at + found;
            message.splice(start..start + placeholder.len(), value.iter().copied());
            at = start + value.len();
        }
    }
    message
}

pub fn text(key: &[u8], english: &[u8], values: &[(Vec<u8>, Vec<u8>)]) -> Vec<u8> {
    let message = STRINGS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_ref()
        .and_then(|strings| strings.get(key).cloned())
        .unwrap_or_else(|| english.to_vec());
    substitute(message, values)
}

fn pairs(values: &[u8]) -> Vec<(Vec<u8>, Vec<u8>)> {
    let mut parts = values.split(|byte| *byte == 0);
    let mut pairs = Vec::new();
    while let (Some(name), Some(value)) = (parts.next(), parts.next()) {
        pairs.push((name.to_vec(), value.to_vec()));
    }
    pairs
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_set_strings(json: *const c_char, length: usize) {
    set_strings(bytes(json, length));
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_load_strings(documents: *const c_char, length: usize) {
    let file = join_path(bytes(documents, length), STRINGS_FILE);
    let contents = read_text(path(file.as_ptr().cast(), file.len())).unwrap_or_default();
    set_strings(&contents);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_text(
    key: *const c_char,
    key_length: usize,
    english: *const c_char,
    english_length: usize,
    values: *const c_char,
    values_length: usize,
) -> AstralRustString {
    AstralRustString::new(text(
        bytes(key, key_length),
        bytes(english, english_length),
        &pairs(bytes(values, values_length)),
    ))
}
