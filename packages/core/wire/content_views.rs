use super::tag::Op;
use std::collections::HashMap;
use std::sync::Mutex;

use super::format::{Span, batch_of, child, wire_parent_of};
use super::identifiers::accessibility_id;

const CONTENT_PREFIX: &[u8] = b"message.content:";
const MAX_CONTENT_VIEWS: usize = 4096;
const CONTENT_DEPTH: usize = 3;

static CONTENT_VIEWS: Mutex<Option<HashMap<i32, u64>>> = Mutex::new(None);

fn with_views<T>(work: impl FnOnce(&mut HashMap<i32, u64>) -> T) -> Option<T> {
    let mut views = CONTENT_VIEWS.lock().ok()?;
    Some(work(views.get_or_insert_with(HashMap::new)))
}

fn content_id(bytes: *const u8, span: &Span) -> Option<Option<u64>> {
    let data = batch_of(bytes, span);
    let identifier = accessibility_id(data, span);
    if span.op == Op::Update && identifier.is_none() {
        return None;
    }
    Some(identifier.and_then(|identifier| super::ids::prefixed(identifier, CONTENT_PREFIX)))
}

pub(crate) fn observe(bytes: *const u8, spans: &[Span]) {
    with_views(|views| {
        for span in spans {
            if span.op == Op::Remove {
                let data = batch_of(bytes, span);
                for position in 0..span.child_count {
                    views.remove(&child(data, span, position));
                }
                continue;
            }
            if span.op != Op::Create && span.op != Op::Update {
                continue;
            }
            match content_id(bytes, span) {
                Some(Some(message)) => {
                    if views.len() >= MAX_CONTENT_VIEWS && !views.contains_key(&span.id) {
                        views.clear();
                    }
                    views.insert(span.id, message);
                }
                Some(None) => {
                    views.remove(&span.id);
                }
                None => {}
            }
        }
    });
}

fn batch_message(bytes: *const u8, spans: &[Span], parent: i32) -> Option<Option<u64>> {
    spans
        .iter()
        .rev()
        .filter(|span| span.id == parent && (span.op == Op::Create || span.op == Op::Update))
        .find_map(|span| content_id(bytes, span))
}

pub(crate) fn view_message(bytes: *const u8, spans: &[Span], view: i32) -> Option<u64> {
    match batch_message(bytes, spans, view) {
        Some(message) => message,
        None => with_views(|views| views.get(&view).copied()).flatten(),
    }
}

fn parent_of(bytes: *const u8, spans: &[Span], id: i32) -> i32 {
    match unsafe { wire_parent_of(bytes, spans.as_ptr(), spans.len(), id) } {
        0 => super::children::wire_discord_parent(id),
        parent => parent,
    }
}

pub(crate) fn message_of(bytes: *const u8, spans: &[Span], id: i32) -> Option<u64> {
    let mut view = parent_of(bytes, spans, id);
    for _ in 0..CONTENT_DEPTH {
        if view == 0 {
            return None;
        }
        if let Some(message) = view_message(bytes, spans, view) {
            return Some(message);
        }
        view = parent_of(bytes, spans, view);
    }
    None
}
