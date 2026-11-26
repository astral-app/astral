use core::ffi::{c_char, c_int};
use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::sync::Mutex;

use crate::support::bridge::{AstralRustString, bytes, path};

const FILE: &str = "plugin-trace";
const MAGIC: [u8; 8] = *b"astrltr1";
const MAX_DEPTH: usize = 16;

#[repr(C)]
struct Record {
    magic: [u8; 8],
    started: i64,
    active: [u8; 128],
    last: [u8; 128],
    running: [u8; 3840],
}

const _: () = assert!(core::mem::size_of::<Record>() == 4112);

#[derive(Clone)]
struct Previous {
    started: i64,
    active: Vec<u8>,
    last: Vec<u8>,
    running: Vec<Vec<u8>>,
}

struct Trace {
    record: *mut Record,
    previous: Option<Previous>,
    stack: Vec<Vec<u8>>,
    depth: usize,
}

unsafe impl Send for Trace {}

static TRACE: Mutex<Trace> = Mutex::new(Trace {
    record: core::ptr::null_mut(),
    previous: None,
    stack: Vec::new(),
    depth: 0,
});

#[repr(C)]
pub struct AstralPreviousSession {
    pub started: i64,
    pub active: AstralRustString,
    pub last: AstralRustString,
    pub running: AstralRustString,
}

fn identifier_character(character: u8) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, b'.' | b'-' | b'_')
}

fn until_nul(value: &[u8]) -> &[u8] {
    &value[..value.iter().position(|byte| *byte == 0).unwrap_or(value.len())]
}

fn field(value: &[u8]) -> Vec<u8> {
    let text = until_nul(value);
    if text.iter().all(|character| identifier_character(*character)) {
        text.to_vec()
    } else {
        Vec::new()
    }
}

fn list(value: &[u8]) -> Vec<Vec<u8>> {
    let text = until_nul(value);
    if !text.iter().all(|character| *character == b',' || identifier_character(*character)) {
        return Vec::new();
    }
    text.split(|byte| *byte == b',')
        .filter(|part| !part.is_empty())
        .map(<[u8]>::to_vec)
        .collect()
}

fn put(destination: &mut [u8], value: &[u8]) {
    let length = value.len().min(destination.len() - 1);
    destination[..length].copy_from_slice(&value[..length]);
    destination[length..].fill(0);
}

fn read(record: &Record) -> Option<Previous> {
    if record.magic != MAGIC {
        return None;
    }
    let session = Previous {
        started: record.started,
        active: field(&record.active),
        last: field(&record.last),
        running: list(&record.running),
    };
    (!session.active.is_empty() || !session.last.is_empty() || !session.running.is_empty()).then_some(session)
}

fn lock() -> std::sync::MutexGuard<'static, Trace> {
    TRACE.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn give(previous: Option<Previous>, into: *mut AstralPreviousSession) -> c_int {
    let Some(previous) = previous else { return 0 };
    if let Some(into) = unsafe { into.as_mut() } {
        *into = AstralPreviousSession {
            started: previous.started,
            active: AstralRustString::new(previous.active),
            last: AstralRustString::new(previous.last),
            running: AstralRustString::list(&previous.running),
        };
    }
    1
}

fn map(directory: &std::path::Path) -> Option<*mut Record> {
    let file = CString::new(directory.join(FILE).as_os_str().as_bytes()).ok()?;
    let descriptor = unsafe { libc::open(file.as_ptr(), libc::O_RDWR | libc::O_CREAT | libc::O_CLOEXEC, 0o600) };
    if descriptor < 0 {
        return None;
    }
    let size = core::mem::size_of::<Record>();
    if unsafe { libc::ftruncate(descriptor, size as libc::off_t) } != 0 {
        unsafe { libc::close(descriptor) };
        return None;
    }
    let mapped = unsafe {
        libc::mmap(
            core::ptr::null_mut(),
            size,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            descriptor,
            0,
        )
    };
    unsafe { libc::close(descriptor) };
    (mapped != libc::MAP_FAILED).then_some(mapped.cast())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_trace_open(directory: *const c_char, length: usize, now: i64, into: *mut AstralPreviousSession) -> c_int {
    let mut trace = lock();
    if !trace.record.is_null() {
        return give(trace.previous.clone(), into);
    }
    let Some(record) = map(path(directory, length)) else { return 0 };
    trace.record = record;
    let mapped = unsafe { &mut *record };
    trace.previous = read(mapped);
    unsafe { core::ptr::write_bytes(record, 0, 1) };
    mapped.magic = MAGIC;
    mapped.started = now;
    trace.depth = 0;
    give(trace.previous.clone(), into)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_trace_previous(into: *mut AstralPreviousSession) -> c_int {
    give(lock().previous.clone(), into)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_trace_enter(plugin: *const c_char, length: usize) {
    let plugin = bytes(plugin, length);
    let mut trace = lock();
    let depth = trace.depth;
    if depth < MAX_DEPTH {
        trace.stack.resize(depth, Vec::new());
        trace.stack.push(plugin.to_vec());
    }
    trace.depth += 1;
    if let Some(record) = unsafe { trace.record.as_mut() } {
        put(&mut record.active, plugin);
        put(&mut record.last, plugin);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_trace_leave() {
    let mut trace = lock();
    if trace.depth == 0 {
        return;
    }
    trace.depth -= 1;
    let depth = trace.depth;
    let outer = if depth > 0 && depth <= MAX_DEPTH {
        trace.stack.get(depth - 1).cloned().unwrap_or_default()
    } else {
        Vec::new()
    };
    if let Some(record) = unsafe { trace.record.as_mut() } {
        put(&mut record.active, &outer);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_trace_set_running(plugins: *const c_char, length: usize) {
    let trace = lock();
    let Some(record) = (unsafe { trace.record.as_mut() }) else { return };
    let mut joined = Vec::new();
    for plugin in bytes(plugins, length).split(|byte| *byte == 0).filter(|plugin| !plugin.is_empty()) {
        if joined.len() + plugin.len() + 1 >= record.running.len() {
            break;
        }
        if !joined.is_empty() {
            joined.push(b',');
        }
        joined.extend_from_slice(plugin);
    }
    put(&mut record.running, &joined);
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_trace_close() {
    let mut trace = lock();
    if !trace.record.is_null() {
        unsafe { libc::munmap(trace.record.cast(), core::mem::size_of::<Record>()) };
    }
    trace.record = core::ptr::null_mut();
    trace.previous = None;
    trace.depth = 0;
}
