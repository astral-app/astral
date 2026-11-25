use core::ffi::{c_char, c_int};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::sync::Mutex;

use crate::bridge::{AstralRustString, path};

static LATEST: Mutex<Option<(i64, Vec<u8>)>> = Mutex::new(None);

pub fn is_out_of_space(error: i32) -> bool {
    error == libc::ENOSPC || error == libc::EDQUOT
}

pub fn note_write_failure(error: i32, path: &Path, at: i64) {
    if !is_out_of_space(error) {
        return;
    }
    let file = path.file_name().map_or_else(Vec::new, |name| name.as_bytes().to_vec());
    *LATEST.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some((at, file));
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_is_out_of_space(error: c_int) -> c_int {
    c_int::from(is_out_of_space(error))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_note_write_failure(error: c_int, data: *const c_char, length: usize, at: i64) {
    note_write_failure(error, path(data, length), at);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_disk_full_note(at: *mut i64, file: *mut AstralRustString) -> c_int {
    let latest = LATEST.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
    let Some((when, name)) = latest else { return 0 };
    if let Some(at) = unsafe { at.as_mut() } {
        *at = when;
    }
    if let Some(file) = unsafe { file.as_mut() } {
        *file = AstralRustString::new(name);
    }
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_clear_disk_full_note() {
    *LATEST.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
}
