use core::ffi::CStr;
use core::ffi::{c_char, c_int, c_void};

unsafe extern "C" {
    fn realloc(pointer: *mut c_void, size: usize) -> *mut c_void;
}

const INITIAL_CAPACITY: usize = 4096;

#[repr(C)]
pub struct Out {
    pub bytes: *mut u8,
    pub length: usize,
    pub capacity: usize,
    pub failed: c_int,
}

impl Out {
    fn reserve(&mut self, more: usize) -> bool {
        if self.failed != 0 {
            return false;
        }
        let Some(needed) = self.length.checked_add(more) else {
            self.failed = 1;
            return false;
        };
        if needed <= self.capacity {
            return true;
        }
        let mut capacity = if self.capacity == 0 { INITIAL_CAPACITY } else { self.capacity };
        while capacity < needed {
            let Some(doubled) = capacity.checked_mul(2) else {
                self.failed = 1;
                return false;
            };
            capacity = doubled;
        }
        let grown = unsafe { realloc(self.bytes.cast(), capacity) };
        if grown.is_null() {
            self.failed = 1;
            return false;
        }
        self.bytes = grown.cast();
        self.capacity = capacity;
        true
    }

    pub fn put(&mut self, data: &[u8]) {
        if !self.reserve(data.len()) {
            return;
        }
        unsafe { core::ptr::copy_nonoverlapping(data.as_ptr(), self.bytes.add(self.length), data.len()) };
        self.length += data.len();
    }

    pub fn put8(&mut self, value: u8) {
        self.put(&[value]);
    }

    pub fn put16(&mut self, value: u16) {
        self.put(&value.to_le_bytes());
    }

    pub fn put32(&mut self, value: u32) {
        self.put(&value.to_le_bytes());
    }

    pub fn put_string(&mut self, text: &[u8]) {
        self.put32(text.len() as u32);
        self.put(text);
    }
}

fn out<'a>(pointer: *mut Out) -> Option<&'a mut Out> {
    unsafe { pointer.as_mut() }
}

fn c_bytes<'a>(text: *const c_char) -> &'a [u8] {
    if text.is_null() {
        return &[];
    }
    unsafe { CStr::from_ptr(text) }.to_bytes()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wirePut(pointer: *mut Out, bytes: *const c_void, length: usize) {
    let Some(out) = out(pointer) else { return };
    if length == 0 {
        return;
    }
    if bytes.is_null() {
        out.failed = 1;
        return;
    }
    out.put(unsafe { core::slice::from_raw_parts(bytes.cast::<u8>(), length) });
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wirePut8(pointer: *mut Out, value: u8) {
    if let Some(out) = out(pointer) {
        out.put8(value);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wirePut16(pointer: *mut Out, value: u16) {
    if let Some(out) = out(pointer) {
        out.put16(value);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wirePut32(pointer: *mut Out, value: u32) {
    if let Some(out) = out(pointer) {
        out.put32(value);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wirePropCount(pointer: *mut Out, count: u16) {
    unsafe { wirePut16(pointer, count) };
}

fn property(out: &mut Out, property: u16, tag: u8) {
    out.put16(property);
    out.put8(tag);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wirePropString(pointer: *mut Out, id: u16, value: *const c_char) {
    let Some(out) = out(pointer) else { return };
    property(out, id, 6);
    out.put_string(c_bytes(value));
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wirePropNumber(pointer: *mut Out, id: u16, value: f64) {
    let Some(out) = out(pointer) else { return };
    property(out, id, 5);
    out.put(&value.to_le_bytes());
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wirePropInt(pointer: *mut Out, id: u16, value: i32) {
    let Some(out) = out(pointer) else { return };
    property(out, id, 3);
    out.put(&value.to_le_bytes());
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wirePropLong(pointer: *mut Out, id: u16, value: i64) {
    let Some(out) = out(pointer) else { return };
    property(out, id, 4);
    out.put(&value.to_le_bytes());
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wirePropBool(pointer: *mut Out, id: u16, value: c_int) {
    let Some(out) = out(pointer) else { return };
    property(out, id, if value != 0 { 2 } else { 1 });
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wirePropQuad(pointer: *mut Out, id: u16, a: f32, b: f32, c: f32, d: f32) {
    let Some(out) = out(pointer) else { return };
    property(out, id, 9);
    for value in [a, b, c, d] {
        out.put(&value.to_le_bytes());
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wirePropEvents(pointer: *mut Out, id: u16, event: *const c_char) {
    let Some(out) = out(pointer) else { return };
    property(out, id, 7);
    out.put32(1);
    out.put8(6);
    out.put_string(c_bytes(event));
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wirePutChildren(pointer: *mut Out, parent: i32, children: *const i32, count: u32) {
    let Some(out) = out(pointer) else { return };
    out.put8(3);
    out.put(&parent.to_le_bytes());
    out.put32(count);
    if count == 0 {
        return;
    }
    if children.is_null() {
        out.failed = 1;
        return;
    }
    for child in unsafe { core::slice::from_raw_parts(children, count as usize) } {
        out.put(&child.to_le_bytes());
    }
}
