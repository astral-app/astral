use core::ffi::{c_char, c_int};
use std::collections::HashMap;
use std::sync::Mutex;

use super::cursor::Cursor;
use super::format::{MAX_SPANS, Span, scan};
use super::properties;

const MAX_IDENTIFIERS: usize = 2048;
const MAX_IDENTIFIER: usize = 112;
const ROUTABLE: [&[u8]; 2] = [b"astral.", b"settings.row.astral"];

static NAMED: Mutex<Option<HashMap<i32, Vec<u8>>>> = Mutex::new(None);

fn routable(text: &[u8]) -> bool {
    ROUTABLE.iter().any(|prefix| text.starts_with(prefix))
}

fn accessibility_id<'a>(bytes: &'a [u8], span: &Span) -> Option<&'a [u8]> {
    let wanted = properties::current().accessibilityId;
    let mut cursor = Cursor::new(bytes, span.start + 5);
    if span.op == 1 {
        let length = cursor.read16() as usize;
        cursor.skip(length);
    }
    let count = cursor.read16();
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

pub fn note(bytes: &[u8]) {
    if bytes.first() != Some(&1) {
        return;
    }
    let mut spans = vec![Span::default(); MAX_SPANS];
    let Some(count) = scan(bytes, &mut spans) else { return };
    let Ok(mut named) = NAMED.lock() else { return };
    let named = named.get_or_insert_with(HashMap::new);
    for span in spans[..count].iter().filter(|span| span.op == 1 || span.op == 2) {
        let Some(text) = accessibility_id(&bytes[..span.end], span) else {
            continue;
        };
        if routable(text) && text.len() < MAX_IDENTIFIER {
            if named.len() >= MAX_IDENTIFIERS
                && !named.contains_key(&span.id)
                && let Some(evicted) = named.keys().next().copied()
            {
                named.remove(&evicted);
            }
            named.insert(span.id, text.to_vec());
        } else {
            named.remove(&span.id);
        }
    }
}

pub fn identifier(id: i32) -> Option<Vec<u8>> {
    NAMED.lock().ok()?.as_ref()?.get(&id).cloned()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_wire_note_identifiers(bytes: *const u8, length: usize) {
    if bytes.is_null() || length == 0 {
        return;
    }
    note(unsafe { core::slice::from_raw_parts(bytes, length) });
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_wire_identifier(id: i32, out: *mut c_char, capacity: usize) -> c_int {
    let Some(text) = identifier(id) else { return 0 };
    if out.is_null() || capacity == 0 || text.is_empty() {
        return 0;
    }
    let length = text.len().min(capacity - 1);
    unsafe {
        core::ptr::copy_nonoverlapping(text.as_ptr(), out.cast::<u8>(), length);
        *out.add(length) = 0;
    }
    1
}
