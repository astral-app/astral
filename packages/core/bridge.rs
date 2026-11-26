use core::ffi::c_char;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

#[repr(C)]
pub struct AstralRustString {
    pub bytes: *mut u8,
    pub length: usize,
}

impl AstralRustString {
    pub fn new(bytes: Vec<u8>) -> Self {
        let boxed = bytes.into_boxed_slice();
        let length = boxed.len();
        AstralRustString {
            bytes: Box::into_raw(boxed).cast::<u8>(),
            length,
        }
    }

    pub fn frames(entries: &[Vec<u8>]) -> Self {
        let mut joined = Vec::new();
        for entry in entries {
            joined.extend_from_slice(&(entry.len() as u32).to_le_bytes());
            joined.extend_from_slice(entry);
        }
        AstralRustString::new(joined)
    }

    pub fn list(entries: &[Vec<u8>]) -> Self {
        let mut joined = Vec::new();
        for entry in entries {
            joined.extend_from_slice(entry);
            joined.push(0);
        }
        AstralRustString::new(joined)
    }
}

pub fn take(string: AstralRustString) -> Vec<u8> {
    if string.bytes.is_null() {
        return Vec::new();
    }
    unsafe { Box::from_raw(core::ptr::slice_from_raw_parts_mut(string.bytes, string.length)) }.into_vec()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_rust_string_free(string: AstralRustString) {
    if string.bytes.is_null() {
        return;
    }
    drop(unsafe { Box::from_raw(core::ptr::slice_from_raw_parts_mut(string.bytes, string.length)) });
}

pub fn bytes<'a>(data: *const c_char, length: usize) -> &'a [u8] {
    if data.is_null() || length == 0 {
        return &[];
    }
    unsafe { core::slice::from_raw_parts(data.cast::<u8>(), length) }
}

pub fn path<'a>(data: *const c_char, length: usize) -> &'a Path {
    Path::new(OsStr::from_bytes(bytes(data, length)))
}

pub fn text(data: *const c_char, length: usize) -> String {
    String::from_utf8_lossy(bytes(data, length)).into_owned()
}

pub unsafe fn give(found: Option<Vec<u8>>, into: *mut AstralRustString) -> bool {
    let Some(found) = found else { return false };
    if let Some(into) = unsafe { into.as_mut() } {
        *into = AstralRustString::new(found);
    }
    true
}

pub fn epoch_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as i64)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_rust_string_from(data: *const c_char, length: usize) -> AstralRustString {
    AstralRustString::new(bytes(data, length).to_vec())
}

pub fn optional(found: Option<Vec<u8>>) -> AstralRustString {
    found.map_or(
        AstralRustString {
            bytes: core::ptr::null_mut(),
            length: 0,
        },
        AstralRustString::new,
    )
}
