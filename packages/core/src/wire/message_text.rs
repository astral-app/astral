use core::ffi::{CStr, c_char, c_int};
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::format::{Span, batch_of, find_string, property_cursor, spans_of, type_is, wireCreation, wireParentOf};
use super::out::{AstralWireBuffer, Out};
use super::properties;
use super::tween::wireTweenFade;

pub type TextProvider = unsafe extern "C" fn(u64, *mut c_char, usize, *mut c_char, usize) -> c_int;
pub type SpanTransformer = unsafe extern "C" fn(u64, *const u8, usize, *mut *mut u8, *mut usize) -> c_int;

unsafe extern "C" {
    fn wireInlineTime(message: u64, text: *mut c_char, capacity: usize) -> usize;
    fn wireMarkRefreshBusy();
    fn free(pointer: *mut core::ffi::c_void);
}

const MAX_TRACKED: usize = 512;
const MAX_OVERRIDE: usize = 8192;
const MAX_NOTE: usize = 64;
const MAX_TIME: usize = 48;
const NOTE_COLOR: u32 = 0x99FFFFFF;
const TIME_SCALE: f64 = 0.75;
const FNV_OFFSET: u64 = 1469598103934665603;
const FNV_PRIME: u64 = 1099511628211;

#[derive(Default)]
struct TrackedText {
    node: i32,
    original: Vec<u8>,
    applied: u64,
    overridden: bool,
    transformed: Option<(u64, Vec<u8>)>,
}

static TEXTS: Mutex<Option<HashMap<u64, TrackedText>>> = Mutex::new(None);
static PROVIDER: AtomicUsize = AtomicUsize::new(0);
static TRANSFORMER: AtomicUsize = AtomicUsize::new(0);

#[unsafe(no_mangle)]
pub extern "C" fn astral_wire_set_message_text_provider(provider: Option<TextProvider>) {
    PROVIDER.store(provider.map_or(0, |function| function as usize), Ordering::Release);
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_wire_set_span_transformer(transformer: Option<SpanTransformer>) {
    TRANSFORMER.store(transformer.map_or(0, |function| function as usize), Ordering::Release);
}

fn provider() -> Option<TextProvider> {
    let raw = PROVIDER.load(Ordering::Acquire);
    (raw != 0).then(|| unsafe { core::mem::transmute::<usize, TextProvider>(raw) })
}

fn transformer() -> Option<SpanTransformer> {
    let raw = TRANSFORMER.load(Ordering::Acquire);
    (raw != 0).then(|| unsafe { core::mem::transmute::<usize, SpanTransformer>(raw) })
}

fn fnv(hash: u64, bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .fold(hash, |hash, byte| (hash ^ u64::from(*byte)).wrapping_mul(FNV_PRIME))
}

fn hash_bytes(bytes: &[u8]) -> u64 {
    let hash = fnv(FNV_OFFSET, bytes);
    if hash == 0 { 1 } else { hash }
}

fn hash_of(text: &[u8], note: &[u8]) -> u64 {
    let hash = fnv((fnv(FNV_OFFSET, text) ^ 0xff).wrapping_mul(FNV_PRIME), note);
    if hash == 0 { 1 } else { hash }
}

fn with_texts<T>(work: impl FnOnce(&mut HashMap<u64, TrackedText>) -> T) -> Option<T> {
    let mut texts = TEXTS.lock().ok()?;
    Some(work(texts.get_or_insert_with(HashMap::new)))
}

fn text_for(texts: &mut HashMap<u64, TrackedText>, message: u64) -> &mut TrackedText {
    if texts.len() >= MAX_TRACKED
        && !texts.contains_key(&message)
        && let Some(evicted) = texts.keys().next().copied()
    {
        texts.remove(&evicted);
    }
    texts.entry(message).or_default()
}

fn content_message(bytes: *const u8, spans: &[Span], id: i32) -> u64 {
    let parent = unsafe { wireParentOf(bytes, spans.as_ptr(), spans.len(), id) };
    if parent == 0 {
        return 0;
    }
    let Some(creation) = (unsafe { wireCreation(spans.as_ptr(), spans.len(), parent).as_ref() }) else {
        return 0;
    };
    let Some(identifier) = find_string(batch_of(bytes, creation), creation, properties::current().accessibilityId) else {
        return 0;
    };
    super::ids::prefixed(identifier, b"message.content:").unwrap_or(0)
}

fn spans_property(bytes: &[u8], span: &Span) -> Option<Vec<u8>> {
    let wanted = properties::current().spans;
    let (mut cursor, count) = property_cursor(bytes, span);
    for _ in 0..count {
        if cursor.failed() {
            break;
        }
        let property = cursor.read16();
        let start = cursor.offset();
        let tag = cursor.read8();
        cursor.skip_value(tag, 0);
        if property == wanted && !cursor.failed() {
            return Some(bytes[start..cursor.offset()].to_vec());
        }
    }
    None
}

fn put_time_span(out: &mut Out, time: &[u8]) {
    let properties = properties::current();
    let mut spaced = b"   ".to_vec();
    spaced.extend_from_slice(time);
    out.put8(8);
    out.put32(3);
    out.prop_string(properties.text, &spaced);
    out.put16(properties.color);
    out.put8(4);
    out.put(&i64::from(NOTE_COLOR).to_le_bytes());
    out.prop_number(properties.scale, TIME_SCALE);
}

fn put_override(out: &mut Out, node: i32, text: &[u8], note: &[u8], time: &[u8]) {
    let properties = properties::current();
    out.update(node, 1);
    out.put16(properties.spans);
    out.put8(7);
    out.put32(if note.is_empty() { 1 } else { 2 } + u32::from(!time.is_empty()));
    out.put8(8);
    out.put32(1);
    out.prop_string(properties.text, text);
    if !note.is_empty() {
        let mut suffix = b" (".to_vec();
        suffix.extend_from_slice(note);
        suffix.push(b')');
        out.put8(8);
        out.put32(2);
        out.prop_string(properties.text, &suffix);
        out.put16(properties.color);
        out.put8(4);
        out.put(&i64::from(NOTE_COLOR).to_le_bytes());
    }
    if !time.is_empty() {
        put_time_span(out, time);
    }
}

fn put_spans_with_time(out: &mut Out, node: i32, spans: &[u8], time: &[u8]) -> bool {
    if spans.len() < 5 || spans[0] != 7 {
        return false;
    }
    let count = u32::from_le_bytes([spans[1], spans[2], spans[3], spans[4]]);
    out.update(node, 1);
    out.put16(properties::current().spans);
    out.put8(7);
    out.put32(count + 1);
    out.put(&spans[5..]);
    put_time_span(out, time);
    true
}

fn put_spans(out: &mut Out, node: i32, spans: &[u8]) {
    out.update(node, 1);
    out.put16(properties::current().spans);
    out.put(spans);
}

fn start_fade(out: &mut Out, node: i32) {
    out.update(node, 1);
    out.prop_number(properties::current().opacity, 0.0);
    wireTweenFade(node);
}

fn provided_text(message: u64) -> Option<(Vec<u8>, Vec<u8>)> {
    let provider = provider()?;
    let mut text = vec![0 as c_char; MAX_OVERRIDE];
    let mut note = vec![0 as c_char; MAX_NOTE];
    if unsafe { provider(message, text.as_mut_ptr(), MAX_OVERRIDE, note.as_mut_ptr(), MAX_NOTE) } == 0 {
        return None;
    }
    text[MAX_OVERRIDE - 1] = 0;
    note[MAX_NOTE - 1] = 0;
    let text = unsafe { CStr::from_ptr(text.as_ptr()) }.to_bytes().to_vec();
    let note = unsafe { CStr::from_ptr(note.as_ptr()) }.to_bytes().to_vec();
    (!text.is_empty()).then_some((text, note))
}

fn inline_time(message: u64) -> Vec<u8> {
    let mut time = [0 as c_char; MAX_TIME];
    unsafe { wireInlineTime(message, time.as_mut_ptr(), MAX_TIME) };
    time[MAX_TIME - 1] = 0;
    unsafe { CStr::from_ptr(time.as_ptr()) }.to_bytes().to_vec()
}

fn transformed_spans(text: &mut TrackedText, message: u64) -> Option<Vec<u8>> {
    let transformer = transformer()?;
    if text.original.is_empty() {
        return None;
    }
    let mut result: *mut u8 = core::ptr::null_mut();
    let mut length = 0;
    let outcome = unsafe { transformer(message, text.original.as_ptr(), text.original.len(), &mut result, &mut length) };
    let produced = (!result.is_null()).then(|| unsafe { core::slice::from_raw_parts(result, length) }.to_vec());
    if !result.is_null() {
        unsafe { free(result.cast()) };
    }
    let source = hash_bytes(&text.original);
    match (outcome, produced) {
        (outcome, _) if outcome < 0 => {
            unsafe { wireMarkRefreshBusy() };
            text.transformed
                .as_ref()
                .filter(|(from, _)| *from == source)
                .map(|(_, spans)| spans.clone())
        }
        (outcome, Some(spans)) if outcome > 0 => {
            text.transformed = Some((source, spans.clone()));
            Some(spans)
        }
        _ => {
            text.transformed = None;
            None
        }
    }
}

fn apply(out: &mut Out, text: &mut TrackedText, message: u64, force: bool) -> bool {
    let provided = provided_text(message);
    let time = inline_time(message);
    let transformed = if provided.is_none() {
        transformed_spans(text, message)
    } else {
        None
    };
    if provided.is_some() || !time.is_empty() || transformed.is_some() {
        let base = transformed.clone().unwrap_or_else(|| text.original.clone());
        let hash = match (&provided, &transformed) {
            (Some((value, note)), _) => hash_of(value, note),
            (None, Some(spans)) => hash_bytes(spans),
            _ => 2,
        };
        let hash = (hash ^ hash_of(&time, b"")) | 1;
        if !force && text.applied == hash {
            return false;
        }
        let override_changed = provided.is_some() != text.overridden;
        match &provided {
            Some((value, note)) => put_override(out, text.node, value, note, &time),
            None if !time.is_empty() => {
                if !put_spans_with_time(out, text.node, &base, &time) {
                    return false;
                }
            }
            None => put_spans(out, text.node, &base),
        }
        if !force && override_changed {
            start_fade(out, text.node);
        }
        text.applied = hash;
        text.overridden = provided.is_some();
        return true;
    }
    if text.applied == 0 || text.original.is_empty() {
        return false;
    }
    put_spans(out, text.node, &text.original);
    if !force && text.overridden {
        start_fade(out, text.node);
    }
    text.applied = 0;
    text.overridden = false;
    true
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wireApplyMessageText(out: *mut Out, message: u64) {
    let Some(out) = (unsafe { out.as_mut() }) else { return };
    if message == 0 {
        return;
    }
    with_texts(|texts| {
        if let Some(text) = texts.get_mut(&message)
            && text.node != 0
            && !text.original.is_empty()
        {
            apply(out, text, message, false);
        }
    });
}

fn tracked_message(texts: &HashMap<u64, TrackedText>, node: i32) -> Option<u64> {
    texts.iter().find(|(_, text)| text.node == node).map(|(message, _)| *message)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wireMessageTexts(
    bytes: *const u8,
    pointer: *const Span,
    count: usize,
    out: *mut Out,
    changed: *mut c_int,
    length: usize,
) {
    let (Some(out), Some(changed)) = (unsafe { out.as_mut() }, unsafe { changed.as_mut() }) else {
        return;
    };
    let spans = spans_of(pointer, count);
    let mut update = Out::default();
    let mut any = false;
    with_texts(|texts| {
        for span in spans {
            let data = batch_of(bytes, span);
            let message = if span.op == 1 && type_is(data, span, b"text") {
                let message = content_message(bytes, spans, span.id);
                if message == 0 {
                    continue;
                }
                let text = text_for(texts, message);
                text.node = span.id;
                text.applied = 0;
                message
            } else if span.op == 2 {
                let Some(message) = tracked_message(texts, span.id) else { continue };
                if let Some(text) = texts.get_mut(&message) {
                    text.applied = 0;
                }
                message
            } else {
                continue;
            };
            let Some(text) = texts.get_mut(&message) else { continue };
            let Some(original) = spans_property(data, span) else { continue };
            text.original = original;
            any |= apply(&mut update, text, message, true);
        }
    });
    if any && update.failed == 0 {
        if *changed == 0 {
            out.put(unsafe { core::slice::from_raw_parts(bytes, length) });
        }
        *changed = 1;
        out.put(unsafe { core::slice::from_raw_parts(update.bytes, update.length) });
    }
    update.discard();
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_wire_refresh_message_texts(buffer: *mut AstralWireBuffer) -> c_int {
    let Some(buffer) = (unsafe { buffer.as_mut() }) else { return 0 };
    *buffer = AstralWireBuffer {
        bytes: core::ptr::null_mut(),
        length: 0,
    };
    let mut out = Out::batch();
    let any = with_texts(|texts| {
        let mut any = false;
        for (message, text) in texts.iter_mut() {
            if text.node != 0 {
                any |= apply(&mut out, text, *message, false);
            }
        }
        any
    })
    .unwrap_or(false);
    if !any {
        out.discard();
        return 0;
    }
    out.finish(buffer) as c_int
}
