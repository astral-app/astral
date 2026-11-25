use core::ffi::c_int;
use std::sync::atomic::{AtomicU32, Ordering};

use super::cursor::{Cursor, MAX_DEPTH};
use super::out::{AstralWireBuffer, Out};
use super::rewriter::{ImageRewriter, Slot};

const CALL_HEADER: usize = 16;
const CALL_KIND: u8 = 1;

static REWRITER: Slot = Slot::new();
static REWRITTEN: AtomicU32 = AtomicU32::new(0);

#[unsafe(no_mangle)]
pub extern "C" fn astral_host_calls_set_image_rewriter(rewriter: Option<ImageRewriter>) {
    REWRITER.set(rewriter);
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_host_calls_rewritten() -> u32 {
    REWRITTEN.load(Ordering::Relaxed)
}

fn copy_value(cursor: &mut Cursor, bytes: &[u8], out: &mut Out, depth: u32, changed: &mut bool) -> bool {
    if depth > MAX_DEPTH || cursor.failed() {
        return false;
    }
    let start = cursor.offset();
    let tag = cursor.read8();
    match tag {
        6 => {
            let length = cursor.read32() as usize;
            let Some(text) = cursor.take(length) else { return false };
            if text.starts_with(b"https://")
                && text.len() > 8
                && let Some(rewritten) = REWRITER.rewrite(text)
            {
                out.put8(6);
                out.put_string(&rewritten);
                *changed = true;
                return true;
            }
            out.put(&bytes[start..cursor.offset()]);
            true
        }
        7 | 8 | 10 => {
            let count = cursor.read32();
            out.put8(tag);
            out.put32(count);
            for _ in 0..count {
                if cursor.failed() {
                    return false;
                }
                if tag == 8 {
                    out.put16(cursor.read16());
                } else if tag == 10 {
                    let key_length = cursor.read32() as usize;
                    let Some(key) = cursor.take(key_length) else { return false };
                    out.put_string(key);
                }
                if !copy_value(cursor, bytes, out, depth + 1, changed) {
                    return false;
                }
            }
            !cursor.failed()
        }
        _ => {
            cursor.skip_value(tag, depth);
            if cursor.failed() {
                return false;
            }
            out.put(&bytes[start..cursor.offset()]);
            true
        }
    }
}

pub fn rewrite(frame: &[u8]) -> Option<Out> {
    REWRITER.get()?;
    if frame.len() < CALL_HEADER || frame[0] != CALL_KIND {
        return None;
    }
    let arguments_length = u32::from_le_bytes(frame[12..16].try_into().ok()?) as usize;
    if arguments_length == 0 || arguments_length > frame.len() - CALL_HEADER {
        return None;
    }
    let end = CALL_HEADER + arguments_length;
    let arguments_bytes = &frame[..end];
    let mut cursor = Cursor::new(arguments_bytes, CALL_HEADER);
    let mut arguments = Out::default();
    let mut changed = false;
    while !cursor.at_end() {
        if !copy_value(&mut cursor, arguments_bytes, &mut arguments, 0, &mut changed) {
            break;
        }
    }
    if !changed || cursor.failed() || cursor.offset() != end || arguments.failed != 0 {
        arguments.discard();
        return None;
    }
    let mut out = Out::default();
    out.put(&frame[..12]);
    out.put32(arguments.length as u32);
    out.put(unsafe { core::slice::from_raw_parts(arguments.bytes, arguments.length) });
    out.put(&frame[end..]);
    arguments.discard();
    Some(out)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_host_call_rewrite(frame: *const u8, length: usize, buffer: *mut AstralWireBuffer) -> c_int {
    let Some(buffer) = (unsafe { buffer.as_mut() }) else { return 0 };
    *buffer = AstralWireBuffer {
        bytes: core::ptr::null_mut(),
        length: 0,
    };
    if frame.is_null() {
        return 0;
    }
    let Some(out) = rewrite(unsafe { core::slice::from_raw_parts(frame, length) }) else {
        return 0;
    };
    if !out.finish(buffer) {
        return 0;
    }
    REWRITTEN.fetch_add(1, Ordering::Relaxed);
    1
}
