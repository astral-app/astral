use core::ffi::{CStr, c_char, c_int};
use std::ffi::CString;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::cursor::Cursor;
use super::format::{Span, batch_of, find_string, spans_of};
use super::out::{AstralWireBuffer, Out};
use super::properties;

pub type SendTransform = unsafe extern "C" fn(u64, *const c_char, *mut c_char, usize) -> c_int;

unsafe extern "C" {}

const EVENT_HEADER: usize = 8;
const MAX_TEXT: usize = 32768;
const EVENT_FRAME: u8 = 1;
const OBJECT_TAG: u8 = 8;
const STRING_TAG: u8 = 6;
const INT_TAG: u8 = 3;

#[derive(Clone, Default)]
struct ComposerText {
    node: i32,
    kind: u16,
    text_key: u16,
    count_key: u16,
    count: i32,
    text: Vec<u8>,
}

#[derive(Default)]
struct Composer {
    typed: ComposerText,
    send_node: i32,
    input_node: i32,
    value: Vec<u8>,
}

static COMPOSER: Mutex<Option<Composer>> = Mutex::new(None);
static TRANSFORM: AtomicUsize = AtomicUsize::new(0);

fn with_composer<T>(work: impl FnOnce(&mut Composer) -> T) -> Option<T> {
    let mut composer = COMPOSER.lock().ok()?;
    Some(work(composer.get_or_insert_with(Composer::default)))
}

fn transform() -> Option<SendTransform> {
    let raw = TRANSFORM.load(Ordering::Acquire);
    (raw != 0).then(|| unsafe { core::mem::transmute::<usize, SendTransform>(raw) })
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_wire_set_send_transform(function: Option<SendTransform>) {
    TRANSFORM.store(function.map_or(0, |function| function as usize), Ordering::Release);
}

fn note_value(composer: &mut Composer, bytes: &[u8], span: &Span) {
    let Some(value) = find_string(bytes, span, properties::current().value) else {
        return;
    };
    composer.value = if value.len() < MAX_TEXT { value.to_vec() } else { Vec::new() };
}

#[unsafe(export_name = "wireTrackComposer")]
pub unsafe extern "C" fn wire_track_composer(bytes: *const u8, pointer: *const Span, count: usize) {
    let accessibility = properties::current().accessibility_id;
    with_composer(|composer| {
        for span in spans_of(pointer, count).iter().filter(|span| span.op == 1 || span.op == 2) {
            let data = batch_of(bytes, span);
            let identifier = find_string(data, span, accessibility);
            if identifier == Some(b"send") {
                composer.send_node = span.id;
            }
            if span.op == 1 && identifier == Some(b"composer") {
                composer.input_node = span.id;
                composer.value.clear();
            }
            if span.id == composer.input_node {
                note_value(composer, data, span);
            }
        }
    });
}

fn event_node(event: &[u8]) -> i32 {
    i32::from_le_bytes([event[2], event[3], event[4], event[5]])
}

fn event_kind(event: &[u8]) -> u16 {
    u16::from_le_bytes([event[6], event[7]])
}

fn typed_text(event: &[u8]) -> Option<ComposerText> {
    let mut cursor = Cursor::new(event, EVENT_HEADER);
    if cursor.read8() != OBJECT_TAG {
        return None;
    }
    let fields = cursor.read32();
    let mut next = ComposerText {
        node: event_node(event),
        kind: event_kind(event),
        count: -1,
        ..ComposerText::default()
    };
    let mut saw_text = false;
    for _ in 0..fields {
        if cursor.failed() {
            break;
        }
        let key = cursor.read16();
        let tag = cursor.read8();
        if tag == STRING_TAG && !saw_text {
            let size = cursor.read32() as usize;
            if size >= MAX_TEXT {
                return None;
            }
            next.text = cursor.take(size)?.to_vec();
            next.text_key = key;
            saw_text = true;
        } else if tag == INT_TAG {
            next.count_key = key;
            next.count = cursor.read32() as i32;
        } else {
            cursor.skip_value(tag, 0);
        }
    }
    (saw_text && next.count >= 0 && !cursor.failed()).then_some(next)
}

fn change_text(typed: &ComposerText, text: &[u8]) -> Out {
    let mut out = Out::default();
    out.put8(EVENT_FRAME);
    out.put8(EVENT_FRAME);
    out.put(&typed.node.to_le_bytes());
    out.put16(typed.kind);
    out.put8(OBJECT_TAG);
    out.put32(2);
    out.put16(typed.text_key);
    out.put8(STRING_TAG);
    out.put_string(text);
    out.put16(typed.count_key);
    out.put8(INT_TAG);
    out.put(&(typed.count + 1).to_le_bytes());
    out
}

fn rewritten(channel: u64, text: &[u8]) -> Option<Vec<u8>> {
    let transform = transform()?;
    let original = CString::new(text).ok()?;
    let mut buffer = vec![0 as c_char; MAX_TEXT];
    if unsafe { transform(channel, original.as_ptr(), buffer.as_mut_ptr(), MAX_TEXT) } == 0 {
        return None;
    }
    buffer[MAX_TEXT - 1] = 0;
    let result = unsafe { CStr::from_ptr(buffer.as_ptr()) }.to_bytes();
    (result != text).then(|| result.to_vec())
}

fn composer_event(event: &[u8]) -> Option<Out> {
    if event.len() < EVENT_HEADER || event[0] != EVENT_FRAME || event[1] != EVENT_FRAME {
        return None;
    }
    let properties = properties::current();
    let kind = event_kind(event);
    let node = event_node(event);
    if kind == properties.change_text {
        with_composer(|composer| {
            if node == composer.input_node
                && let Some(typed) = typed_text(event)
            {
                composer.typed = typed;
            }
        });
        return None;
    }
    transform()?;
    let typed = with_composer(|composer| {
        let ready = kind == properties.press
            && composer.send_node != 0
            && node == composer.send_node
            && composer.input_node != 0
            && composer.typed.node == composer.input_node
            && composer.typed.text == composer.value;
        ready.then(|| composer.typed.clone())
    })??;
    let text = rewritten(super::ids::current_channel(), &typed.text)?;
    Some(change_text(&typed, &text))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_wire_composer_event(event: *const u8, length: usize, before: *mut AstralWireBuffer) -> c_int {
    let Some(before) = (unsafe { before.as_mut() }) else { return 0 };
    *before = AstralWireBuffer {
        bytes: core::ptr::null_mut(),
        length: 0,
    };
    if event.is_null() {
        return 0;
    }
    match composer_event(unsafe { core::slice::from_raw_parts(event, length) }) {
        Some(out) => c_int::from(out.finish(before)),
        None => 0,
    }
}

#[unsafe(export_name = "wireComposerNode")]
pub extern "C" fn wire_composer_node() -> i32 {
    with_composer(|composer| composer.input_node).unwrap_or(0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_wire_composer_text(text: *mut c_char, capacity: usize) -> usize {
    if text.is_null() || capacity == 0 {
        return 0;
    }
    let value = with_composer(|composer| composer.value.clone()).unwrap_or_default();
    let length = value.len().min(capacity - 1);
    unsafe {
        core::ptr::copy_nonoverlapping(value.as_ptr(), text.cast::<u8>(), length);
        *text.add(length) = 0;
    }
    length
}
