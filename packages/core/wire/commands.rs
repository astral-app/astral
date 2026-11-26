use core::ffi::{CStr, c_char, c_int};
use std::sync::Mutex;

use super::out::{AstralWireBuffer, Out};
use super::properties;

unsafe extern "C" {
    #[link_name = "wireRowOfMessage"]
    fn wire_row_of_message(message: u64, list: *mut i32, index: *mut i64) -> c_int;
    #[link_name = "wireComposerNode"]
    fn wire_composer_node() -> i32;
}

static PENDING: Mutex<Vec<u8>> = Mutex::new(Vec::new());

fn queue(command: &[u8]) {
    if let Ok(mut pending) = PENDING.lock() {
        pending.extend_from_slice(command);
    }
}

fn alignment(align: *const c_char) -> &'static [u8] {
    if align.is_null() {
        return b"visible";
    }
    match unsafe { CStr::from_ptr(align) }.to_bytes() {
        b"start" => b"start",
        b"end" => b"end",
        _ => b"visible",
    }
}

fn command(build: impl FnOnce(&mut Out)) -> bool {
    let mut out = Out::default();
    build(&mut out);
    let built = out.failed == 0;
    if built {
        queue(unsafe { core::slice::from_raw_parts(out.bytes, out.length) });
    }
    out.discard();
    built
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_wire_scroll_to_message(message: u64, animated: c_int, align: *const c_char) -> c_int {
    let mut list = 0;
    let mut index = 0;
    if message == 0 || unsafe { wire_row_of_message(message, &mut list, &mut index) } == 0 || !(0..=i64::from(i32::MAX)).contains(&index) {
        return 0;
    }
    let properties = properties::current();
    let align = alignment(align);
    c_int::from(command(|out| {
        out.put8(7);
        out.put(&list.to_le_bytes());
        out.put8(8);
        out.put32(4);
        out.prop_int(properties.index, index as i32);
        out.prop_string(properties.align, align);
        out.prop_number(properties.offset, 0.0);
        out.put16(properties.animated);
        out.put8(if animated != 0 { 2 } else { 1 });
    }))
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_wire_focus_composer(focus: c_int) -> c_int {
    let composer = unsafe { wire_composer_node() };
    if focus != 0 && composer == 0 {
        return 0;
    }
    c_int::from(command(|out| {
        out.put8(6);
        out.put8(u8::from(focus != 0));
        if focus != 0 {
            out.put(&composer.to_le_bytes());
        }
    }))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_wire_take_commands(buffer: *mut AstralWireBuffer) -> c_int {
    let Some(buffer) = (unsafe { buffer.as_mut() }) else { return 0 };
    *buffer = AstralWireBuffer {
        bytes: core::ptr::null_mut(),
        length: 0,
    };
    let pending = match PENDING.lock() {
        Ok(mut pending) => core::mem::take(&mut *pending),
        Err(_) => return 0,
    };
    if pending.is_empty() {
        return 0;
    }
    let mut out = Out::batch();
    out.put(&pending);
    c_int::from(out.finish(buffer))
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_wire_message_author(message: u64) -> u64 {
    super::decorate::author_of(message)
}
