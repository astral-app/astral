use core::ffi::c_char;

use crate::bridge::AstralRustString;
use crate::wire::out::AstralWireBuffer;
use crate::wire::rewrite::astral_wire_buffer_free;

pub fn raw(text: &str) -> (*const c_char, usize) {
    (text.as_ptr().cast(), text.len())
}

pub fn take(string: AstralRustString) -> String {
    String::from_utf8_lossy(&string.into_bytes()).into_owned()
}

pub fn take_optional(string: AstralRustString) -> Option<String> {
    (!string.bytes.is_null()).then(|| take(string))
}

pub fn take_json(string: AstralRustString) -> serde_json::Value {
    serde_json::from_str(&take(string)).unwrap_or(serde_json::Value::Null)
}

pub struct WireBuffer(pub AstralWireBuffer);

impl WireBuffer {
    pub fn new() -> WireBuffer {
        WireBuffer(AstralWireBuffer {
            bytes: core::ptr::null_mut(),
            length: 0,
        })
    }

    pub fn pointer(&mut self) -> *mut AstralWireBuffer {
        &raw mut self.0
    }

    pub fn bytes(&self) -> &[u8] {
        if self.0.bytes.is_null() {
            return &[];
        }
        unsafe { core::slice::from_raw_parts(self.0.bytes, self.0.length) }
    }
}

impl Drop for WireBuffer {
    fn drop(&mut self) {
        unsafe { astral_wire_buffer_free(self.pointer()) };
    }
}

const MIN_BATCH: usize = 2;

pub fn append(merged: &mut Vec<u8>, buffer: &WireBuffer) {
    let bytes = buffer.bytes();
    if bytes.len() < MIN_BATCH {
        return;
    }
    if merged.is_empty() {
        merged.extend_from_slice(bytes);
    } else {
        merged.extend_from_slice(&bytes[1..]);
    }
}

pub fn collect(merged: &mut Vec<u8>, produce: impl FnOnce(*mut AstralWireBuffer) -> core::ffi::c_int) {
    let mut buffer = WireBuffer::new();
    if produce(buffer.pointer()) != 0 {
        append(merged, &buffer);
    }
}
