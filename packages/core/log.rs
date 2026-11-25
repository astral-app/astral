use core::ffi::{c_char, c_int, c_void};
use std::sync::{Arc, Mutex};

use crate::bridge::{AstralRustString, bytes};

pub const DEBUG: c_int = 0;
pub const INFO: c_int = 1;
pub const WARNING: c_int = 2;
pub const ERROR: c_int = 3;
pub const DEFAULT_CATEGORY: &str = "default";

const CATEGORIES: [&str; 12] = [
    "default",
    "runtime",
    "updater",
    "themes",
    "fonts",
    "plugins",
    "settings",
    "toolbox",
    "hooks",
    "filesystem",
    "utilities",
    "debugger",
];

pub type LogCallback = unsafe extern "C" fn(*mut c_void, c_int, *const c_char, usize, *const c_char, usize);
pub type SystemLogCallback = unsafe extern "C" fn(*mut c_void, c_int, *const c_char, usize);
pub type Release = unsafe extern "C" fn(*mut c_void);

struct Sink<F> {
    callback: F,
    context: *mut c_void,
    release: Option<Release>,
}

unsafe impl<F> Send for Sink<F> {}
unsafe impl<F> Sync for Sink<F> {}

impl<F> Drop for Sink<F> {
    fn drop(&mut self) {
        if let Some(release) = self.release {
            unsafe { release(self.context) };
        }
    }
}

static SINK: Mutex<Option<Arc<Sink<LogCallback>>>> = Mutex::new(None);
static SYSTEM_SINK: Mutex<Option<Arc<Sink<SystemLogCallback>>>> = Mutex::new(None);

fn current<F>(slot: &Mutex<Option<Arc<Sink<F>>>>) -> Option<Arc<Sink<F>>> {
    slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
}

fn replace<F>(slot: &Mutex<Option<Arc<Sink<F>>>>, sink: Option<Sink<F>>) {
    let previous = core::mem::replace(
        &mut *slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()),
        sink.map(Arc::new),
    );
    drop(previous);
}

pub fn log(level: c_int, category: &str, message: &str) {
    if let Some(sink) = current(&SINK) {
        unsafe {
            (sink.callback)(
                sink.context,
                level,
                category.as_ptr().cast(),
                category.len(),
                message.as_ptr().cast(),
                message.len(),
            )
        };
    }
}

pub fn write_system(level: c_int, line: &[u8]) {
    if let Some(sink) = current(&SYSTEM_SINK) {
        unsafe { (sink.callback)(sink.context, level, line.as_ptr().cast(), line.len()) };
    }
}

pub fn is_category(category: &[u8]) -> bool {
    CATEGORIES.iter().any(|known| known.as_bytes() == category)
}

pub fn level_from_index(index: c_int) -> c_int {
    if matches!(index, DEBUG | WARNING | ERROR) { index } else { INFO }
}

pub fn format_line(category: &[u8], message: &[u8]) -> Vec<u8> {
    let mut line = Vec::with_capacity(category.len() + message.len() + 3);
    line.push(b'[');
    line.extend_from_slice(category);
    line.extend_from_slice(b"] ");
    line.extend_from_slice(message);
    line
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_set_log_sink(callback: Option<LogCallback>, context: *mut c_void, release: Option<Release>) {
    replace(
        &SINK,
        callback.map(|callback| Sink {
            callback,
            context,
            release,
        }),
    );
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_set_system_log_sink(callback: Option<SystemLogCallback>, context: *mut c_void, release: Option<Release>) {
    replace(
        &SYSTEM_SINK,
        callback.map(|callback| Sink {
            callback,
            context,
            release,
        }),
    );
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_write_system_log(level: c_int, line: *const c_char, length: usize) {
    write_system(level, bytes(line, length));
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_log(
    level: c_int,
    category: *const c_char,
    category_length: usize,
    message: *const c_char,
    message_length: usize,
) {
    if let Some(sink) = current(&SINK) {
        unsafe { (sink.callback)(sink.context, level, category, category_length, message, message_length) };
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_is_log_category(category: *const c_char, length: usize) -> c_int {
    c_int::from(is_category(bytes(category, length)))
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_log_level_from_index(index: c_int) -> c_int {
    level_from_index(index)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_format_log_line(
    category: *const c_char,
    category_length: usize,
    message: *const c_char,
    message_length: usize,
) -> AstralRustString {
    AstralRustString::new(format_line(bytes(category, category_length), bytes(message, message_length)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_log_from_platform(
    level: c_int,
    category: *const c_char,
    category_length: usize,
    message: *const c_char,
    message_length: usize,
) {
    let category = bytes(category, category_length);
    let message = bytes(message, message_length);
    let level = level_from_index(level);
    if is_category(category) {
        unsafe {
            astral_log(
                level,
                category.as_ptr().cast(),
                category.len(),
                message.as_ptr().cast(),
                message.len(),
            )
        };
        return;
    }
    let line = format_line(category, message);
    unsafe {
        astral_log(
            level,
            DEFAULT_CATEGORY.as_ptr().cast(),
            DEFAULT_CATEGORY.len(),
            line.as_ptr().cast(),
            line.len(),
        )
    };
}
