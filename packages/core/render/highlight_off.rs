use core::ffi::c_char;

use serde_json::Value;

use crate::support::bridge::AstralRustString;

pub fn highlight(_code: &str, _language: &str, _theme: &[u8]) -> Value {
    Value::Null
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_code_highlight(
    _code: *const c_char,
    _code_length: usize,
    _language: *const c_char,
    _language_length: usize,
    _theme: *const c_char,
    _theme_length: usize,
) -> AstralRustString {
    AstralRustString::new(Value::Null.to_string().into_bytes())
}
