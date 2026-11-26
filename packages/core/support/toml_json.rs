use core::ffi::c_char;

use crate::support::bridge::{AstralRustString, bytes};

fn json(value: toml::Value) -> serde_json::Value {
    match value {
        toml::Value::String(text) => serde_json::Value::String(text),
        toml::Value::Integer(integer) => serde_json::Value::from(integer),
        toml::Value::Float(number) => serde_json::Number::from_f64(number).map_or(serde_json::Value::Null, serde_json::Value::Number),
        toml::Value::Boolean(flag) => serde_json::Value::Bool(flag),
        toml::Value::Datetime(_) => serde_json::Value::Null,
        toml::Value::Array(entries) => serde_json::Value::Array(entries.into_iter().map(json).collect()),
        toml::Value::Table(table) => serde_json::Value::Object(table.into_iter().map(|(key, value)| (key, json(value))).collect()),
    }
}

pub fn parse(text: &str) -> Option<serde_json::Value> {
    text.parse::<toml::Table>().ok().map(|table| json(toml::Value::Table(table)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_parse_toml(text: *const c_char, length: usize) -> AstralRustString {
    let parsed = std::str::from_utf8(bytes(text, length)).ok().and_then(parse);
    AstralRustString::new(parsed.map_or_else(Vec::new, |value| value.to_string().into_bytes()))
}
