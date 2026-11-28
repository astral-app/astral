use super::tag::Op;
use std::collections::HashMap;
use std::sync::Mutex;

use super::cursor::Cursor;
use super::format::{Span, child, wire_scan};
use super::out::Out;

const MAX_LIST_CHILDREN: usize = 1 << 16;
const EMPTY: i32 = 0;

#[derive(Default)]
struct Lists {
    children: HashMap<i32, Vec<i32>>,
    parents: HashMap<i32, i32>,
}

impl Lists {
    fn get(&self, parent: &i32) -> Option<&Vec<i32>> {
        self.children.get(parent)
    }

    fn insert(&mut self, parent: i32, children: Vec<i32>) {
        self.remove(&parent);
        for child in &children {
            self.parents.insert(*child, parent);
        }
        self.children.insert(parent, children);
    }

    fn remove(&mut self, parent: &i32) {
        let Some(children) = self.children.remove(parent) else { return };
        for child in children {
            if self.parents.get(&child) == Some(parent) {
                self.parents.remove(&child);
            }
        }
    }

    fn clear(&mut self) {
        self.children.clear();
        self.parents.clear();
    }
}

static LISTS: Mutex<Option<Lists>> = Mutex::new(None);

fn with_lists<T>(work: impl FnOnce(&mut Lists) -> T) -> Option<T> {
    let mut lists = LISTS.lock().ok()?;
    Some(work(lists.get_or_insert_with(Lists::default)))
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

fn note(lists: &mut Lists, bytes: &[u8], span: &Span) -> Option<Vec<i32>> {
    match Op::parse(span.op) {
        Some(Op::Create) => {
            lists.remove(&span.id);
        }
        Some(Op::SetChildren) if span.id != EMPTY => {
            lists.insert(span.id, children_at(bytes, span));
        }
        Some(Op::Remove) => {
            for removed in children_at(bytes, span) {
                lists.remove(&removed);
                lists.parents.remove(&removed);
            }
        }
        Some(Op::EditChildren) => {
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
    out.put_op(Op::SetChildren);
    out.put(&parent.to_le_bytes());
    out.put32(children.len() as u32);
    for id in children {
        out.put(&id.to_le_bytes());
    }
}

#[unsafe(export_name = "wireExpandChildEdits")]
pub unsafe extern "C" fn wire_expand_child_edits(
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
    if out.failed != 0 || unsafe { wire_scan(out.bytes, out.length, spans, count) } != 0 {
        out.discard();
        unsafe { wire_scan(bytes, length, spans, count) };
        return core::ptr::null_mut();
    }
    unsafe { *new_length = out.length };
    out.bytes
}

pub fn discord_children(parent: i32) -> Vec<i32> {
    with_lists(|lists| lists.get(&parent).cloned()).flatten().unwrap_or_default()
}

#[unsafe(export_name = "wireDiscordChildren")]
pub unsafe extern "C" fn wire_discord_children(parent: i32, children: *mut *const i32) -> u32 {
    with_lists(|lists| match lists.get(&parent) {
        Some(list) if !children.is_null() => {
            unsafe { *children = list.as_ptr() };
            list.len() as u32
        }
        _ => 0,
    })
    .unwrap_or(0)
}

#[unsafe(export_name = "wireDiscordParent")]
pub extern "C" fn wire_discord_parent(id: i32) -> i32 {
    with_lists(|lists| lists.parents.get(&id).copied().unwrap_or(0)).unwrap_or(0)
}

#[unsafe(export_name = "wireResetChildLists")]
pub extern "C" fn wire_reset_child_lists() {
    with_lists(Lists::clear);
}
