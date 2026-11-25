use core::ffi::CStr;
use core::ffi::{c_char, c_int, c_void};

unsafe extern "C" {
    fn realloc(pointer: *mut c_void, size: usize) -> *mut c_void;
    fn free(pointer: *mut c_void);
}

const INITIAL_CAPACITY: usize = 4096;

#[repr(C)]
pub struct Out {
    pub bytes: *mut u8,
    pub length: usize,
    pub capacity: usize,
    pub failed: c_int,
}

#[repr(C)]
pub struct AstralWireBuffer {
    pub bytes: *mut u8,
    pub length: usize,
}

pub const WIRE_VERSION: u8 = 1;

impl Default for Out {
    fn default() -> Self {
        Out {
            bytes: core::ptr::null_mut(),
            length: 0,
            capacity: 0,
            failed: 0,
        }
    }
}

impl Out {
    pub fn batch() -> Self {
        let mut out = Out::default();
        out.put8(WIRE_VERSION);
        out
    }

    pub fn finish(self, buffer: &mut AstralWireBuffer) -> bool {
        if self.failed != 0 {
            unsafe { free(self.bytes.cast()) };
            return false;
        }
        buffer.bytes = self.bytes;
        buffer.length = self.length;
        true
    }

    pub fn discard(self) {
        unsafe { free(self.bytes.cast()) };
    }

    pub fn prop_count(&mut self, count: u16) {
        self.put16(count);
    }

    pub fn prop_number(&mut self, id: u16, value: f64) {
        self.put16(id);
        self.put8(5);
        self.put(&value.to_le_bytes());
    }

    pub fn prop_int(&mut self, id: u16, value: i32) {
        self.put16(id);
        self.put8(3);
        self.put(&value.to_le_bytes());
    }

    pub fn prop_string(&mut self, id: u16, value: &[u8]) {
        self.put16(id);
        self.put8(6);
        self.put_string(value);
    }

    pub fn prop_quad(&mut self, id: u16, quad: [f32; 4]) {
        self.put16(id);
        self.put8(9);
        for value in quad {
            self.put(&value.to_le_bytes());
        }
    }

    pub fn prop_long(&mut self, id: u16, value: i64) {
        self.put16(id);
        self.put8(4);
        self.put(&value.to_le_bytes());
    }

    pub fn prop_bool(&mut self, id: u16, value: bool) {
        self.put16(id);
        self.put8(if value { 2 } else { 1 });
    }

    pub fn prop_unset(&mut self, id: u16) {
        self.put16(id);
        self.put8(0);
    }

    pub fn prop_events(&mut self, id: u16, event: &[u8]) {
        self.put16(id);
        self.put8(7);
        self.put32(1);
        self.put8(6);
        self.put_string(event);
    }

    pub fn prop_gradient(&mut self, id: u16, angle: f64, colors: &[u32]) {
        let properties = super::properties::current();
        self.put16(id);
        self.put8(8);
        self.put32(3);
        self.prop_number(properties.angle, angle);
        self.put16(properties.colors);
        self.put8(7);
        self.put32(colors.len() as u32);
        for color in colors {
            self.put8(4);
            self.put(&i64::from(*color).to_le_bytes());
        }
        self.put16(properties.positions);
        self.put8(7);
        self.put32(colors.len() as u32);
        for index in 0..colors.len() {
            self.put8(5);
            let position = if colors.len() > 1 {
                index as f64 / (colors.len() - 1) as f64
            } else {
                0.0
            };
            self.put(&position.to_le_bytes());
        }
    }

    pub fn put_children(&mut self, parent: i32, children: &[i32]) {
        self.put8(3);
        self.put(&parent.to_le_bytes());
        self.put32(children.len() as u32);
        for child in children {
            self.put(&child.to_le_bytes());
        }
    }

    pub fn remove(&mut self, nodes: &[i32]) {
        self.put8(4);
        self.put32(nodes.len() as u32);
        for node in nodes {
            self.put(&node.to_le_bytes());
        }
    }

    pub fn create(&mut self, node: i32, kind: &[u8], count: u16) {
        self.put8(1);
        self.put(&node.to_le_bytes());
        self.put16(kind.len() as u16);
        self.put(kind);
        self.prop_count(count);
    }

    pub fn update(&mut self, node: i32, count: u16) {
        self.put8(2);
        self.put(&node.to_le_bytes());
        self.prop_count(count);
    }

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

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wirePropGradient(pointer: *mut Out, id: u16, angle: f64, colors: *const u32, count: u32) {
    let Some(out) = out(pointer) else { return };
    let colors = if colors.is_null() || count == 0 {
        &[][..]
    } else {
        unsafe { core::slice::from_raw_parts(colors, count as usize) }
    };
    out.prop_gradient(id, angle, colors);
}
