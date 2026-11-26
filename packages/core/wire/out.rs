use super::tag::{Op, Tag};
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
        self.put_tag(Tag::Double);
        self.put(&value.to_le_bytes());
    }

    pub fn prop_int(&mut self, id: u16, value: i32) {
        self.put16(id);
        self.put_tag(Tag::Int);
        self.put(&value.to_le_bytes());
    }

    pub fn prop_string(&mut self, id: u16, value: &[u8]) {
        self.put16(id);
        self.put_tag(Tag::String);
        self.put_string(value);
    }

    pub fn prop_quad(&mut self, id: u16, quad: [f32; 4]) {
        self.put16(id);
        self.put_tag(Tag::Quad);
        for value in quad {
            self.put(&value.to_le_bytes());
        }
    }

    pub fn prop_long(&mut self, id: u16, value: i64) {
        self.put16(id);
        self.put_tag(Tag::Long);
        self.put(&value.to_le_bytes());
    }

    pub fn prop_bool(&mut self, id: u16, value: bool) {
        self.put16(id);
        self.put8(if value { 2 } else { 1 });
    }

    pub fn prop_unset(&mut self, id: u16) {
        self.put16(id);
        self.put_tag(Tag::Null);
    }

    pub fn prop_events(&mut self, id: u16, event: &[u8]) {
        self.put16(id);
        self.put_tag(Tag::Array);
        self.put32(1);
        self.put_tag(Tag::String);
        self.put_string(event);
    }

    pub fn prop_gradient(&mut self, id: u16, angle: f64, colors: &[u32]) {
        let properties = super::properties::current();
        self.put16(id);
        self.put_tag(Tag::Map);
        self.put32(3);
        self.prop_number(properties.angle, angle);
        self.put16(properties.colors);
        self.put_tag(Tag::Array);
        self.put32(colors.len() as u32);
        for color in colors {
            self.put_tag(Tag::Long);
            self.put(&i64::from(*color).to_le_bytes());
        }
        self.put16(properties.positions);
        self.put_tag(Tag::Array);
        self.put32(colors.len() as u32);
        for index in 0..colors.len() {
            self.put_tag(Tag::Double);
            let position = if colors.len() > 1 {
                index as f64 / (colors.len() - 1) as f64
            } else {
                0.0
            };
            self.put(&position.to_le_bytes());
        }
    }

    pub fn put_children(&mut self, parent: i32, children: &[i32]) {
        self.put_op(Op::SetChildren);
        self.put(&parent.to_le_bytes());
        self.put32(children.len() as u32);
        for child in children {
            self.put(&child.to_le_bytes());
        }
    }

    pub fn remove(&mut self, nodes: &[i32]) {
        self.put_op(Op::Remove);
        self.put32(nodes.len() as u32);
        for node in nodes {
            self.put(&node.to_le_bytes());
        }
    }

    pub fn create(&mut self, node: i32, kind: &[u8], count: u16) {
        self.put_op(Op::Create);
        self.put(&node.to_le_bytes());
        self.put16(kind.len() as u16);
        self.put(kind);
        self.prop_count(count);
    }

    pub fn update(&mut self, node: i32, count: u16) {
        self.put_op(Op::Update);
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

    pub fn put_tag(&mut self, tag: Tag) {
        self.put8(tag as u8);
    }

    pub fn put_op(&mut self, op: Op) {
        self.put8(op as u8);
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

#[unsafe(export_name = "wirePut")]
pub unsafe extern "C" fn wire_put(pointer: *mut Out, bytes: *const c_void, length: usize) {
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

#[unsafe(export_name = "wirePut8")]
pub unsafe extern "C" fn wire_put8(pointer: *mut Out, value: u8) {
    if let Some(out) = out(pointer) {
        out.put8(value);
    }
}

#[unsafe(export_name = "wirePut16")]
pub unsafe extern "C" fn wire_put16(pointer: *mut Out, value: u16) {
    if let Some(out) = out(pointer) {
        out.put16(value);
    }
}

#[unsafe(export_name = "wirePut32")]
pub unsafe extern "C" fn wire_put32(pointer: *mut Out, value: u32) {
    if let Some(out) = out(pointer) {
        out.put32(value);
    }
}

#[unsafe(export_name = "wirePropCount")]
pub unsafe extern "C" fn wire_prop_count(pointer: *mut Out, count: u16) {
    unsafe { wire_put16(pointer, count) };
}

fn property(out: &mut Out, property: u16, tag: Tag) {
    out.put16(property);
    out.put_tag(tag);
}

#[unsafe(export_name = "wirePropString")]
pub unsafe extern "C" fn wire_prop_string(pointer: *mut Out, id: u16, value: *const c_char) {
    let Some(out) = out(pointer) else { return };
    property(out, id, Tag::String);
    out.put_string(c_bytes(value));
}

#[unsafe(export_name = "wirePropNumber")]
pub unsafe extern "C" fn wire_prop_number(pointer: *mut Out, id: u16, value: f64) {
    let Some(out) = out(pointer) else { return };
    property(out, id, Tag::Double);
    out.put(&value.to_le_bytes());
}

#[unsafe(export_name = "wirePropInt")]
pub unsafe extern "C" fn wire_prop_int(pointer: *mut Out, id: u16, value: i32) {
    let Some(out) = out(pointer) else { return };
    property(out, id, Tag::Int);
    out.put(&value.to_le_bytes());
}

#[unsafe(export_name = "wirePropLong")]
pub unsafe extern "C" fn wire_prop_long(pointer: *mut Out, id: u16, value: i64) {
    let Some(out) = out(pointer) else { return };
    property(out, id, Tag::Long);
    out.put(&value.to_le_bytes());
}

#[unsafe(export_name = "wirePropBool")]
pub unsafe extern "C" fn wire_prop_bool(pointer: *mut Out, id: u16, value: c_int) {
    let Some(out) = out(pointer) else { return };
    property(out, id, Tag::flag(value != 0));
}

#[unsafe(export_name = "wirePropQuad")]
pub unsafe extern "C" fn wire_prop_quad(pointer: *mut Out, id: u16, first: f32, second: f32, third: f32, fourth: f32) {
    let Some(out) = out(pointer) else { return };
    property(out, id, Tag::Quad);
    for value in [first, second, third, fourth] {
        out.put(&value.to_le_bytes());
    }
}

#[unsafe(export_name = "wirePropEvents")]
pub unsafe extern "C" fn wire_prop_events(pointer: *mut Out, id: u16, event: *const c_char) {
    let Some(out) = out(pointer) else { return };
    property(out, id, Tag::Array);
    out.put32(1);
    out.put_tag(Tag::String);
    out.put_string(c_bytes(event));
}

#[unsafe(export_name = "wirePutChildren")]
pub unsafe extern "C" fn wire_put_children(pointer: *mut Out, parent: i32, children: *const i32, count: u32) {
    let Some(out) = out(pointer) else { return };
    out.put_op(Op::SetChildren);
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

#[unsafe(export_name = "wirePropGradient")]
pub unsafe extern "C" fn wire_prop_gradient(pointer: *mut Out, id: u16, angle: f64, colors: *const u32, count: u32) {
    let Some(out) = out(pointer) else { return };
    let colors = if colors.is_null() || count == 0 {
        &[][..]
    } else {
        unsafe { core::slice::from_raw_parts(colors, count as usize) }
    };
    out.prop_gradient(id, angle, colors);
}
