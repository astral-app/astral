use core::ffi::{CStr, c_char, c_int};

use super::cursor::Cursor;
use super::index;

pub const MAX_SPANS: usize = 8192;
const SPAN_HEADER: usize = 1 + 4;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Span {
    pub op: u8,
    pub id: i32,
    pub start: usize,
    pub end: usize,
    pub children: usize,
    pub child_count: u32,
}

#[repr(C)]
pub struct CCursor {
    pub bytes: *const u8,
    pub length: usize,
    pub offset: usize,
    pub failed: c_int,
}

fn batch<'a>(bytes: *const u8, length: usize) -> &'a [u8] {
    if bytes.is_null() || length == 0 {
        return &[];
    }
    unsafe { core::slice::from_raw_parts(bytes, length) }
}

fn spans<'a>(spans: *const Span, count: usize) -> &'a [Span] {
    if spans.is_null() || count == 0 {
        return &[];
    }
    unsafe { core::slice::from_raw_parts(spans, count.min(MAX_SPANS)) }
}

fn span_bytes<'a>(bytes: *const u8, span: &Span) -> &'a [u8] {
    batch(bytes, span.end)
}

fn c_bytes<'a>(text: *const c_char) -> &'a [u8] {
    if text.is_null() {
        return &[];
    }
    unsafe { CStr::from_ptr(text) }.to_bytes()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wireSkipValue(pointer: *mut CCursor, tag: u8, depth: c_int) {
    let Some(state) = (unsafe { pointer.as_mut() }) else { return };
    if state.failed != 0 {
        return;
    }
    let mut cursor = Cursor::new(batch(state.bytes, state.length), state.offset);
    cursor.skip_value(tag, depth.max(0) as u32);
    state.offset = cursor.offset();
    if cursor.failed() {
        state.failed = 1;
    }
}

fn scan_span(cursor: &mut Cursor, span: &mut Span) -> bool {
    span.op = cursor.read8();
    match span.op {
        1 => {
            span.id = cursor.read32() as i32;
            let type_length = cursor.read16() as usize;
            cursor.skip(type_length);
            cursor.skip_properties();
        }
        2 => {
            span.id = cursor.read32() as i32;
            cursor.skip_properties();
        }
        3 => {
            span.id = cursor.read32() as i32;
            span.child_count = cursor.read32();
            span.children = cursor.offset();
            cursor.skip((span.child_count as usize).saturating_mul(4));
        }
        4 | 9 => {
            span.child_count = cursor.read32();
            span.children = cursor.offset();
            cursor.skip((span.child_count as usize).saturating_mul(4));
        }
        5 => cursor.skip(4),
        6 => {
            if cursor.read8() != 0 {
                cursor.skip(4);
            }
        }
        7 => {
            cursor.skip(4);
            cursor.skip_tagged(0);
        }
        8 => {}
        10 => {
            span.id = cursor.read32() as i32;
            let edits = cursor.read32();
            for _ in 0..edits {
                if cursor.failed() {
                    break;
                }
                cursor.skip(8);
                let count = cursor.read32() as usize;
                cursor.skip(count.saturating_mul(4));
            }
        }
        _ => return false,
    }
    true
}

pub fn scan(bytes: &[u8], out: &mut [Span]) -> Option<usize> {
    let mut cursor = Cursor::new(bytes, 0);
    if cursor.read8() != 1 {
        return None;
    }
    let mut used = 0;
    while !cursor.at_end() {
        if used == out.len() {
            return None;
        }
        let mut span = Span {
            start: cursor.offset(),
            ..Span::default()
        };
        if !scan_span(&mut cursor, &mut span) {
            return None;
        }
        span.end = cursor.offset();
        out[used] = span;
        used += 1;
    }
    if cursor.failed() { None } else { Some(used) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wireScan(bytes: *const u8, length: usize, pointer: *mut Span, count: *mut usize) -> c_int {
    if pointer.is_null() || count.is_null() {
        return -1;
    }
    let data = batch(bytes, length);
    let out = unsafe { core::slice::from_raw_parts_mut(pointer, MAX_SPANS) };
    match scan(data, out) {
        Some(used) => {
            index::remember(pointer, used, data, &out[..used]);
            unsafe { *count = used };
            0
        }
        None => {
            index::forget(pointer);
            -1
        }
    }
}

pub fn child(bytes: &[u8], span: &Span, index: u32) -> i32 {
    let start = span.children.saturating_add((index as usize).saturating_mul(4));
    match bytes.get(start..start.saturating_add(4)) {
        Some(value) if index < span.child_count => i32::from_le_bytes([value[0], value[1], value[2], value[3]]),
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wireChild(bytes: *const u8, span: *const Span, index: u32) -> i32 {
    let Some(span) = (unsafe { span.as_ref() }) else { return 0 };
    child(span_bytes(bytes, span), span, index)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wireChildAt(bytes: *const u8, span: *const Span, index: u32) -> i32 {
    unsafe { wireChild(bytes, span, index) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wireChildList(_bytes: *const u8, pointer: *const Span, count: usize, id: i32) -> *const Span {
    if let Some(found) = index::with_index(pointer, count, |index| index.child_list(id)) {
        return found.map_or(core::ptr::null(), |position| unsafe { pointer.add(position) });
    }
    spans(pointer, count)
        .iter()
        .rev()
        .find(|span| span.op == 3 && span.id == id)
        .map_or(core::ptr::null(), |span| span as *const Span)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wireParentOf(bytes: *const u8, pointer: *const Span, count: usize, id: i32) -> i32 {
    if let Some(parent) = index::with_index(pointer, count, |index| index.parent(id)) {
        return parent;
    }
    for span in spans(pointer, count).iter().rev() {
        if span.op != 3 {
            continue;
        }
        let data = span_bytes(bytes, span);
        if (0..span.child_count).any(|position| child(data, span, position) == id) {
            return span.id;
        }
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wireCreation(pointer: *const Span, count: usize, id: i32) -> *const Span {
    if let Some(found) = index::with_index(pointer, count, |index| index.creation(id)) {
        return found.map_or(core::ptr::null(), |position| unsafe { pointer.add(position) });
    }
    spans(pointer, count)
        .iter()
        .find(|span| span.op == 1 && span.id == id)
        .map_or(core::ptr::null(), |span| span as *const Span)
}

fn properties<'a>(bytes: &'a [u8], span: &Span) -> (Cursor<'a>, u16) {
    let mut cursor = Cursor::new(bytes, span.start + SPAN_HEADER);
    let type_length = cursor.read16() as usize;
    cursor.skip(type_length);
    let count = cursor.read16();
    (cursor, count)
}

pub fn property_cursor<'a>(bytes: &'a [u8], span: &Span) -> (Cursor<'a>, u16) {
    if span.op == 1 {
        return properties(bytes, span);
    }
    let mut cursor = Cursor::new(bytes, span.start + SPAN_HEADER);
    let count = cursor.read16();
    (cursor, count)
}

pub fn find_string<'a>(bytes: &'a [u8], span: &Span, wanted: u16) -> Option<&'a [u8]> {
    let (mut cursor, count) = property_cursor(bytes, span);
    for _ in 0..count {
        if cursor.failed() {
            break;
        }
        let property = cursor.read16();
        let tag = cursor.read8();
        if property == wanted && tag == 6 {
            let length = cursor.read32() as usize;
            return cursor.take(length);
        }
        cursor.skip_value(tag, 0);
    }
    None
}

pub fn find_flag(bytes: &[u8], span: &Span, wanted: u16) -> Option<bool> {
    let (mut cursor, count) = property_cursor(bytes, span);
    for _ in 0..count {
        if cursor.failed() {
            break;
        }
        let property = cursor.read16();
        let tag = cursor.read8();
        if property == wanted && (tag == 1 || tag == 2) {
            return Some(tag == 2);
        }
        cursor.skip_value(tag, 0);
    }
    None
}

pub fn batch_of<'a>(bytes: *const u8, span: &Span) -> &'a [u8] {
    span_bytes(bytes, span)
}

pub fn spans_of<'a>(pointer: *const Span, count: usize) -> &'a [Span] {
    spans(pointer, count)
}

fn string_property<'a>(bytes: &'a [u8], span: &Span, wanted: u16) -> Option<&'a [u8]> {
    let (mut cursor, count) = properties(bytes, span);
    for _ in 0..count {
        if cursor.failed() {
            break;
        }
        let property = cursor.read16();
        let tag = cursor.read8();
        if property == wanted && tag == 6 {
            let length = cursor.read32() as usize;
            return cursor.take(length);
        }
        cursor.skip_value(tag, 0);
    }
    None
}

fn matches_string(bytes: *const u8, span: *const Span, wanted: u16, value: *const c_char, prefix: bool) -> c_int {
    let Some(span) = (unsafe { span.as_ref() }) else { return 0 };
    let expected = c_bytes(value);
    let Some(found) = string_property(span_bytes(bytes, span), span, wanted) else {
        return 0;
    };
    let matched = if prefix { found.starts_with(expected) } else { found == expected };
    matched as c_int
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wireStringPropertyPrefix(bytes: *const u8, span: *const Span, wanted: u16, prefix: *const c_char) -> c_int {
    matches_string(bytes, span, wanted, prefix, true)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wireStringProperty(bytes: *const u8, span: *const Span, wanted: u16, value: *const c_char) -> c_int {
    matches_string(bytes, span, wanted, value, false)
}

pub fn type_is(bytes: &[u8], span: &Span, expected: &[u8]) -> bool {
    let mut cursor = Cursor::new(bytes, span.start + SPAN_HEADER);
    let length = cursor.read16() as usize;
    cursor.take(length).is_some_and(|name| name == expected)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wireTypeIs(bytes: *const u8, span: *const Span, expected: *const c_char) -> c_int {
    let Some(span) = (unsafe { span.as_ref() }) else { return 0 };
    type_is(span_bytes(bytes, span), span, c_bytes(expected)) as c_int
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wireColorProperty(bytes: *const u8, span: *const Span, wanted: u16, color: *mut u32) -> c_int {
    let Some(span) = (unsafe { span.as_ref() }) else { return 0 };
    let (mut cursor, count) = properties(span_bytes(bytes, span), span);
    for _ in 0..count {
        if cursor.failed() {
            break;
        }
        let property = cursor.read16();
        let tag = cursor.read8();
        if property == wanted && (tag == 3 || tag == 4) {
            let value = cursor.read32();
            if cursor.failed() || color.is_null() {
                return 0;
            }
            unsafe { *color = value };
            return 1;
        }
        cursor.skip_value(tag, 0);
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wireFirstOfType(
    bytes: *const u8,
    pointer: *const Span,
    count: usize,
    node: i32,
    expected: *const c_char,
    depth: c_int,
) -> i32 {
    if node == 0 {
        return 0;
    }
    let Some(creation) = (unsafe { wireCreation(pointer, count, node).as_ref() }) else {
        return 0;
    };
    if unsafe { wireTypeIs(bytes, creation, expected) } != 0 {
        return node;
    }
    if depth <= 0 {
        return 0;
    }
    let Some(list) = (unsafe { wireChildList(bytes, pointer, count, node).as_ref() }) else {
        return 0;
    };
    let data = span_bytes(bytes, list);
    for position in 0..list.child_count {
        let found = unsafe { wireFirstOfType(bytes, pointer, count, child(data, list, position), expected, depth - 1) };
        if found != 0 {
            return found;
        }
    }
    0
}
