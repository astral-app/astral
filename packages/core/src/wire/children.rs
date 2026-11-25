use std::collections::HashMap;
use std::sync::Mutex;

use super::cursor::Cursor;
use super::format::{Span, child, wireScan};
use super::out::Out;

const MAX_LIST_CHILDREN: usize = 1 << 16;
const EMPTY: i32 = 0;

static LISTS: Mutex<Option<HashMap<i32, Vec<i32>>>> = Mutex::new(None);

fn with_lists<T>(work: impl FnOnce(&mut HashMap<i32, Vec<i32>>) -> T) -> Option<T> {
    let mut lists = LISTS.lock().ok()?;
    Some(work(lists.get_or_insert_with(HashMap::new)))
}

fn children_at(bytes: &[u8], span: &Span) -> Vec<i32> {
    (0..span.child_count).map(|position| child(bytes, span, position)).collect()
}

pub fn apply_edits(bytes: &[u8], span: &Span, previous: &[i32]) -> Option<Vec<i32>> {
    let mut cursor = Cursor::new(bytes, span.start + 5);
    let edits = cursor.read32();
    let mut children = Vec::with_capacity(previous.len() + 16);
    let mut position = 0usize;
    for _ in 0..edits {
        if cursor.failed() {
            return None;
        }
        let keep = cursor.read32() as usize;
        let drop = cursor.read32() as usize;
        let added = cursor.read32() as usize;
        let remaining = previous.len() - position;
        if keep > remaining || drop > remaining - keep || added > MAX_LIST_CHILDREN {
            return None;
        }
        let inserted = cursor.take(added * 4)?;
        children.extend_from_slice(&previous[position..position + keep]);
        position += keep + drop;
        children.extend(inserted.as_chunks::<4>().0.iter().map(|id| i32::from_le_bytes(*id)));
    }
    if cursor.failed() || children.len() + (previous.len() - position) > MAX_LIST_CHILDREN {
        return None;
    }
    children.extend_from_slice(&previous[position..]);
    Some(children)
}

fn note(lists: &mut HashMap<i32, Vec<i32>>, bytes: &[u8], span: &Span) -> Option<Vec<i32>> {
    match span.op {
        1 => {
            lists.remove(&span.id);
        }
        3 if span.id != EMPTY => {
            lists.insert(span.id, children_at(bytes, span));
        }
        4 => {
            for removed in children_at(bytes, span) {
                lists.remove(&removed);
            }
        }
        10 => {
            let previous = lists.get(&span.id).map(Vec::as_slice).unwrap_or(&[]);
            match apply_edits(bytes, span, previous) {
                Some(children) => {
                    if span.id != EMPTY {
                        lists.insert(span.id, children.clone());
                    }
                    return Some(children);
                }
                None => {
                    lists.remove(&span.id);
                }
            }
        }
        _ => {}
    }
    None
}

fn put_children(out: &mut Out, parent: i32, children: &[i32]) {
    out.put8(3);
    out.put(&parent.to_le_bytes());
    out.put32(children.len() as u32);
    for id in children {
        out.put(&id.to_le_bytes());
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wireExpandChildEdits(
    bytes: *const u8,
    length: usize,
    spans: *mut Span,
    count: *mut usize,
    new_length: *mut usize,
) -> *mut u8 {
    if bytes.is_null() || spans.is_null() || count.is_null() || new_length.is_null() {
        return core::ptr::null_mut();
    }
    let data = unsafe { core::slice::from_raw_parts(bytes, length) };
    let scanned = unsafe { core::slice::from_raw_parts(spans, *count) };
    let mut out = Out::default();
    let mut copied = 0;
    let mut expanded = false;
    with_lists(|lists| {
        for span in scanned {
            let Some(children) = note(lists, &data[..span.end], span) else {
                continue;
            };
            out.put(&data[copied..span.start]);
            put_children(&mut out, span.id, &children);
            copied = span.end;
            expanded = true;
        }
    });
    if !expanded {
        out.discard();
        return core::ptr::null_mut();
    }
    out.put(&data[copied..]);
    if out.failed != 0 || unsafe { wireScan(out.bytes, out.length, spans, count) } != 0 {
        out.discard();
        unsafe { wireScan(bytes, length, spans, count) };
        return core::ptr::null_mut();
    }
    unsafe { *new_length = out.length };
    out.bytes
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wireDiscordChildren(parent: i32, children: *mut *const i32) -> u32 {
    with_lists(|lists| match lists.get(&parent) {
        Some(list) if !children.is_null() => {
            unsafe { *children = list.as_ptr() };
            list.len() as u32
        }
        _ => 0,
    })
    .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn wireDiscordParent(id: i32) -> i32 {
    with_lists(|lists| {
        lists
            .iter()
            .find(|(_, children)| children.contains(&id))
            .map_or(0, |(parent, _)| *parent)
    })
    .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn wireResetChildLists() {
    with_lists(HashMap::clear);
}
