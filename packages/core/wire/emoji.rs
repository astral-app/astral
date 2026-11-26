use core::ffi::{CStr, c_char, c_int};
use std::ffi::CString;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::cursor::Cursor;
use super::format::{Span, batch_of, child, spans_of, type_is, wire_child_list, wire_creation, wire_scan};
use super::out::Out;
use super::properties;

pub type EmojiResolver = unsafe extern "C" fn(*const c_char, *mut c_char, usize) -> c_int;

const MAX_URL: usize = 512;
const MAX_CODEPOINTS: usize = 128;
const MAX_DEPTH: u32 = 6;
const SCHEME: &[u8] = b"emoji://";
const REACTION_PREFIX: &[u8] = b"message.reaction:";
const UNICODE_MARKER: &[u8] = b":unicode:";
const GLYPH_CAPACITY: usize = 64;

static RESOLVER: AtomicUsize = AtomicUsize::new(0);

#[unsafe(no_mangle)]
pub extern "C" fn astral_wire_set_emoji_resolver(resolver: Option<EmojiResolver>) {
    RESOLVER.store(resolver.map_or(0, |function| function as usize), Ordering::Release);
}

fn resolver() -> Option<EmojiResolver> {
    let raw = RESOLVER.load(Ordering::Acquire);
    (raw != 0).then(|| unsafe { core::mem::transmute::<usize, EmojiResolver>(raw) })
}

fn resolve(codepoints: &[u8]) -> Option<Vec<u8>> {
    let resolver = resolver()?;
    let codepoints = CString::new(codepoints).ok()?;
    let mut url = [0 as c_char; MAX_URL];
    if unsafe { resolver(codepoints.as_ptr(), url.as_mut_ptr(), MAX_URL) } <= 0 {
        return None;
    }
    url[MAX_URL - 1] = 0;
    Some(unsafe { CStr::from_ptr(url.as_ptr()) }.to_bytes().to_vec())
}

fn resolve_uri(text: &[u8]) -> Option<Vec<u8>> {
    let codepoints = text.strip_prefix(SCHEME)?;
    if codepoints.is_empty() || codepoints.len() >= MAX_CODEPOINTS {
        return None;
    }
    resolve(codepoints)
}

fn copy_tagged(out: &mut Out, cursor: &mut Cursor, key: u16, depth: u32, replaced: &mut bool) -> bool {
    let tag = cursor.read8();
    out.put8(tag);
    copy_value(out, cursor, tag, key, depth, replaced)
}

fn copy_value(out: &mut Out, cursor: &mut Cursor, tag: u8, key: u16, depth: u32, replaced: &mut bool) -> bool {
    if depth > MAX_DEPTH || cursor.failed() {
        return false;
    }
    match tag {
        6 => {
            let length = cursor.read32() as usize;
            let Some(text) = cursor.take(length) else { return false };
            match (key == properties::current().uri).then(|| resolve_uri(text)).flatten() {
                Some(url) => {
                    out.put_string(&url);
                    *replaced = true;
                }
                None => out.put_string(text),
            }
            true
        }
        7 => {
            let count = cursor.read32();
            out.put32(count);
            for _ in 0..count {
                if cursor.failed() {
                    break;
                }
                copy_tagged(out, cursor, 0, depth + 1, replaced);
            }
            !cursor.failed()
        }
        8 => {
            let count = cursor.read32();
            out.put32(count);
            for _ in 0..count {
                if cursor.failed() {
                    break;
                }
                let entry = cursor.read16();
                out.put16(entry);
                copy_tagged(out, cursor, entry, depth + 1, replaced);
            }
            !cursor.failed()
        }
        _ => {
            let start = cursor.offset();
            cursor.skip_value(tag, depth);
            if cursor.failed() {
                return false;
            }
            out.put(&cursor_bytes(cursor)[start..cursor.offset()]);
            true
        }
    }
}

fn cursor_bytes<'a>(cursor: &Cursor<'a>) -> &'a [u8] {
    cursor.bytes()
}

fn contains_emoji(bytes: &[u8], span: &Span) -> bool {
    bytes
        .get(span.start..span.end)
        .is_some_and(|node| node.windows(SCHEME.len()).any(|window| window == SCHEME))
}

fn copy_node(bytes: &[u8], span: &Span, out: &mut Out) -> bool {
    let properties = properties::current();
    let mut cursor = Cursor::new(bytes, span.start + 5);
    if span.op == 1 {
        let type_length = cursor.read16() as usize;
        cursor.skip(type_length);
    }
    if cursor.failed() {
        return false;
    }
    out.put(&bytes[span.start..cursor.offset()]);
    let count = cursor.read16();
    out.put16(count);
    let mut replaced = false;
    for _ in 0..count {
        if cursor.failed() {
            break;
        }
        let property = cursor.read16();
        let tag = cursor.read8();
        out.put16(property);
        out.put8(tag);
        if property == properties.uri || property == properties.spans {
            copy_value(out, &mut cursor, tag, property, 0, &mut replaced);
        } else {
            let at = cursor.offset();
            cursor.skip_value(tag, 0);
            if !cursor.failed() {
                out.put(&bytes[at..cursor.offset()]);
            }
        }
    }
    if !cursor.failed() && cursor.offset() < span.end {
        out.put(&bytes[cursor.offset()..span.end]);
    }
    !cursor.failed() && replaced
}

#[unsafe(export_name = "wireEmojiBatch")]
pub unsafe extern "C" fn wire_emoji_batch(
    bytes: *const u8,
    length: usize,
    spans: *mut Span,
    count: *mut usize,
    new_length: *mut usize,
) -> *mut u8 {
    if resolver().is_none() || bytes.is_null() || spans.is_null() || count.is_null() || new_length.is_null() {
        return core::ptr::null_mut();
    }
    let data = unsafe { core::slice::from_raw_parts(bytes, length) };
    let scanned = unsafe { core::slice::from_raw_parts(spans, *count) };
    let candidate = |span: &&Span| (span.op == 1 || span.op == 2) && contains_emoji(data, span);
    if !scanned.iter().any(|span| candidate(&span)) {
        return core::ptr::null_mut();
    }
    let mut out = Out::default();
    let mut copied = 0;
    let mut replaced = false;
    for span in scanned.iter().filter(candidate) {
        out.put(&data[copied..span.start]);
        let mut node = Out::default();
        if copy_node(&data[..span.end], span, &mut node) && node.failed == 0 {
            out.put(unsafe { core::slice::from_raw_parts(node.bytes, node.length) });
            replaced = true;
        } else {
            out.put(&data[span.start..span.end]);
        }
        node.discard();
        copied = span.end;
    }
    out.put(&data[copied..]);
    let mut checked = 0;
    if !replaced || out.failed != 0 || unsafe { wire_scan(out.bytes, out.length, spans, &mut checked) } != 0 {
        out.discard();
        if replaced {
            unsafe { wire_scan(bytes, length, spans, count) };
        }
        return core::ptr::null_mut();
    }
    unsafe {
        *count = checked;
        *new_length = out.length;
    }
    out.bytes
}

fn reaction_codepoints<'a>(bytes: &'a [u8], span: &Span) -> Option<&'a [u8]> {
    let identifier = super::format::find_string(bytes, span, properties::current().accessibility_id)?;
    let rest = identifier.strip_prefix(REACTION_PREFIX)?;
    if rest.is_empty() {
        return None;
    }
    let at = rest.windows(UNICODE_MARKER.len()).position(|window| window == UNICODE_MARKER)?;
    let points = &rest[at + UNICODE_MARKER.len()..];
    (!points.is_empty() && points.len() < MAX_CODEPOINTS).then_some(points)
}

fn glyph_text(bytes: *const u8, spans: &[Span], id: i32, depth: u32) -> i32 {
    if depth >= 3 {
        return 0;
    }
    let Some(list) = (unsafe { wire_child_list(bytes, spans.as_ptr(), spans.len(), id).as_ref() }) else {
        return 0;
    };
    if list.child_count == 0 {
        return 0;
    }
    let first = child(batch_of(bytes, list), list, 0);
    if let Some(creation) = unsafe { wire_creation(spans.as_ptr(), spans.len(), first).as_ref() }
        && type_is(batch_of(bytes, creation), creation, b"text")
    {
        return first;
    }
    glyph_text(bytes, spans, first, depth + 1)
}

pub fn glyph(codepoints: &[u8], capacity: usize) -> Vec<u8> {
    let mut glyph = Vec::new();
    for part in codepoints.split(|byte| *byte == b'-') {
        if glyph.len() + 4 >= capacity {
            break;
        }
        let Some(point) = core::str::from_utf8(part).ok().and_then(|hex| u32::from_str_radix(hex, 16).ok()) else {
            break;
        };
        match point {
            0..0x80 => glyph.push(point as u8),
            0x80..0x800 => glyph.extend_from_slice(&[0xC0 | (point >> 6) as u8, 0x80 | (point & 0x3F) as u8]),
            0x800..0x10000 => glyph.extend_from_slice(&[
                0xE0 | (point >> 12) as u8,
                0x80 | ((point >> 6) & 0x3F) as u8,
                0x80 | (point & 0x3F) as u8,
            ]),
            _ => glyph.extend_from_slice(&[
                0xF0 | (point >> 18) as u8,
                0x80 | ((point >> 12) & 0x3F) as u8,
                0x80 | ((point >> 6) & 0x3F) as u8,
                0x80 | (point & 0x3F) as u8,
            ]),
        }
    }
    glyph
}

#[unsafe(export_name = "wireGlyphOf")]
pub unsafe extern "C" fn wire_glyph_of(codepoints: *const c_char, out: *mut c_char, capacity: usize) {
    if out.is_null() || capacity == 0 {
        return;
    }
    let points = if codepoints.is_null() {
        &[][..]
    } else {
        unsafe { CStr::from_ptr(codepoints) }.to_bytes()
    };
    let encoded = glyph(points, capacity);
    let length = encoded.len().min(capacity - 1);
    unsafe {
        core::ptr::copy_nonoverlapping(encoded.as_ptr(), out.cast::<u8>(), length);
        *out.add(length) = 0;
    }
}

fn put_reaction_image(out: &mut Out, text: i32, glyph: &[u8], url: &[u8]) {
    let properties = properties::current();
    out.update(text, 1);
    out.put16(properties.spans);
    out.put8(7);
    out.put32(1);
    out.put8(8);
    out.put32(2);
    out.prop_string(properties.text, glyph);
    out.put16(properties.image);
    out.put8(8);
    out.put32(2);
    out.prop_string(properties.uri, url);
    out.prop_number(properties.size, 20.0);
}

#[unsafe(export_name = "wireReactionEmoji")]
pub unsafe extern "C" fn wire_reaction_emoji(
    bytes: *const u8,
    pointer: *const Span,
    count: usize,
    out: *mut Out,
    changed: *mut c_int,
    length: usize,
) {
    if resolver().is_none() {
        return;
    }
    let (Some(out), Some(changed)) = (unsafe { out.as_mut() }, unsafe { changed.as_mut() }) else {
        return;
    };
    let spans = spans_of(pointer, count);
    for span in spans.iter().filter(|span| span.op == 1) {
        let Some(codepoints) = reaction_codepoints(batch_of(bytes, span), span) else {
            continue;
        };
        let text = glyph_text(bytes, spans, span.id, 0);
        if text == 0 {
            continue;
        }
        let Some(url) = resolve(codepoints) else { continue };
        if *changed == 0 {
            out.put(unsafe { core::slice::from_raw_parts(bytes, length) });
        }
        *changed = 1;
        put_reaction_image(out, text, &glyph(codepoints, GLYPH_CAPACITY), &url);
    }
}
