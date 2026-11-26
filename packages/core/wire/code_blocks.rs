use super::tag::{Op, Tag};
use core::ffi::{c_char, c_int};
use std::collections::HashMap;
use std::sync::Mutex;

use serde_json::Value;

use super::abi::Callback;
use super::format::{Span, batch_of, child, find_string, property_cursor, spans_of, type_is, wire_creation, wire_parent_of};
use super::ids::current_channel;
use super::out::{AstralWireBuffer, Out};
use super::patch::wire_mark_refresh_busy;
use super::properties;
use crate::support::bridge::AstralRustString;

const CONTENT_PREFIX: &[u8] = b"message.content:";
const MONOSPACE: &[u8] = b"monospace";
const MAX_BLOCKS: usize = 256;
const MAX_ANCESTORS: usize = 4;

#[repr(C)]
pub struct AstralCodeBlockContext {
    pub message_id: u64,
    pub channel_id: u64,
    pub index: u32,
    pub code: *const c_char,
    pub code_length: usize,
}

pub type CodeBlockDecorator = unsafe extern "C" fn(*const AstralCodeBlockContext, *mut AstralRustString) -> c_int;

#[derive(Default)]
struct Block {
    message: u64,
    index: u32,
    parent: i32,
    code: Vec<u8>,
    card: Option<Value>,
    shown: u64,
    header: Vec<i32>,
    dirty: bool,
}

#[derive(Default)]
struct State {
    blocks: HashMap<i32, Block>,
    parents: HashMap<i32, Vec<i32>>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static DECORATOR: Callback = Callback::new();

fn with_state<T>(work: impl FnOnce(&mut State) -> T) -> T {
    let mut guard = STATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    work(guard.get_or_insert_with(State::default))
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_wire_set_code_block_decorator(decorator: Option<CodeBlockDecorator>) {
    DECORATOR.set(decorator);
}

fn span_texts(bytes: &[u8], span: &Span) -> Option<Vec<u8>> {
    let properties = properties::current();
    let (mut cursor, count) = property_cursor(bytes, span);
    for _ in 0..count {
        if cursor.failed() {
            break;
        }
        let property = cursor.read16();
        let tag = cursor.read8();
        if property != properties.spans || tag != Tag::Array {
            cursor.skip_value(tag, 0);
            continue;
        }
        let mut text = Vec::new();
        for _ in 0..cursor.read32() {
            if cursor.read8() != Tag::Map {
                return None;
            }
            for _ in 0..cursor.read32() {
                let field = cursor.read16();
                let kind = cursor.read8();
                if field == properties.text && kind == Tag::String {
                    let length = cursor.read32() as usize;
                    text.extend_from_slice(cursor.take(length)?);
                } else {
                    cursor.skip_value(kind, 0);
                }
            }
        }
        return (!cursor.failed()).then_some(text);
    }
    None
}

pub fn is_code_block(bytes: &[u8], span: &Span) -> bool {
    let properties = properties::current();
    span.op == Op::Create
        && type_is(bytes, span, b"text")
        && find_string(bytes, span, properties.font_family) == Some(MONOSPACE)
        && has_property(bytes, span, properties.background)
}

fn has_property(bytes: &[u8], span: &Span, wanted: u16) -> bool {
    let (mut cursor, count) = property_cursor(bytes, span);
    for _ in 0..count {
        if cursor.failed() {
            return false;
        }
        let property = cursor.read16();
        if property == wanted {
            return true;
        }
        let tag = cursor.read8();
        cursor.skip_value(tag, 0);
    }
    false
}

fn message_of(bytes: *const u8, spans: &[Span], node: i32) -> (u64, i32) {
    let parent = unsafe { wire_parent_of(bytes, spans.as_ptr(), spans.len(), node) };
    let mut current = parent;
    for _ in 0..MAX_ANCESTORS {
        if current == 0 {
            break;
        }
        if let Some(creation) = unsafe { wire_creation(spans.as_ptr(), spans.len(), current).as_ref() }
            && let Some(identifier) = find_string(batch_of(bytes, creation), creation, properties::current().accessibility_id)
            && let Some(message) = super::ids::prefixed(identifier, CONTENT_PREFIX)
        {
            return (message, parent);
        }
        current = unsafe { wire_parent_of(bytes, spans.as_ptr(), spans.len(), current) };
    }
    (0, parent)
}

fn hash(value: &Value) -> u64 {
    value.to_string().bytes().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

fn put_card(out: &mut Out, block: &mut Block, card: &Value) {
    if !block.header.is_empty() {
        out.remove(&block.header);
        block.header.clear();
    }
    let mut created = Vec::new();
    if super::rich::put_tree(out, &mut created, card).is_some() {
        block.header = created;
    }
}

fn put_parent(out: &mut Out, state: &State, parent: i32) {
    let Some(children) = state.parents.get(&parent) else { return };
    let mut merged = Vec::with_capacity(children.len() + 2);
    for node in children {
        let card = state
            .blocks
            .get(node)
            .filter(|block| block.card.is_some())
            .and_then(|block| block.header.first());
        merged.push(card.copied().unwrap_or(*node));
    }
    out.put_children(parent, &merged);
}

fn decorate(block: &mut Block) -> Option<Value> {
    let decorator = DECORATOR.get::<CodeBlockDecorator>()?;
    let context = AstralCodeBlockContext {
        message_id: block.message,
        channel_id: current_channel(),
        index: block.index,
        code: block.code.as_ptr().cast(),
        code_length: block.code.len(),
    };
    let mut card = AstralRustString {
        bytes: core::ptr::null_mut(),
        length: 0,
    };
    let outcome = unsafe { decorator(&context, &mut card) };
    let produced = (!card.bytes.is_null()).then(|| unsafe { core::slice::from_raw_parts(card.bytes, card.length) }.to_vec());
    unsafe { crate::support::bridge::astral_rust_string_free(card) };
    if outcome < 0 {
        wire_mark_refresh_busy();
        return block.card.clone();
    }
    if outcome == 0 {
        return None;
    }
    produced
        .and_then(|produced| serde_json::from_slice(&produced).ok())
        .filter(|tree: &Value| tree.is_object() || tree.is_array())
}

fn render(out: &mut Out, state: &mut State, node: i32, force: bool) -> bool {
    let Some(block) = state.blocks.get_mut(&node) else { return false };
    let card = decorate(block);
    let Some(card) = card else {
        if block.card.take().is_none() {
            return false;
        }
        out.remove(&block.header);
        block.header.clear();
        block.shown = 0;
        let parent = block.parent;
        put_parent(out, state, parent);
        return true;
    };
    let shown = hash(&card);
    if !force && shown == block.shown {
        return false;
    }
    put_card(out, block, &card);
    block.card = Some(card);
    block.shown = shown;
    let parent = block.parent;
    put_parent(out, state, parent);
    true
}

fn note_children(state: &mut State, bytes: &[u8], span: &Span) {
    let children: Vec<i32> = (0..span.child_count).map(|position| child(bytes, span, position)).collect();
    let mut index = 0;
    for node in &children {
        if let Some(block) = state.blocks.get_mut(node) {
            block.index = index;
            index += 1;
        }
    }
    state.parents.insert(span.id, children);
}

fn forget(state: &mut State, node: i32) {
    if let Some(block) = state.blocks.remove(&node) {
        let still_used = state.blocks.values().any(|other| other.parent == block.parent);
        if !still_used {
            state.parents.remove(&block.parent);
        }
    }
    state.parents.remove(&node);
}

#[unsafe(export_name = "wireCodeBlocks")]
pub unsafe extern "C" fn wire_code_blocks(
    bytes: *const u8,
    pointer: *const Span,
    count: usize,
    out: *mut Out,
    changed: *mut c_int,
    length: usize,
) {
    if DECORATOR.get::<CodeBlockDecorator>().is_none() {
        return;
    }
    let (Some(out), Some(changed)) = (unsafe { out.as_mut() }, unsafe { changed.as_mut() }) else {
        return;
    };
    let spans = spans_of(pointer, count);
    let mut update = Out::default();
    let mut any = false;
    with_state(|state| {
        let mut touched = Vec::new();
        let mut relisted = Vec::new();
        for span in spans {
            let data = batch_of(bytes, span);
            match Op::parse(span.op) {
                Some(Op::Remove) => {
                    for position in 0..span.child_count {
                        let removed = child(data, span, position);
                        if let Some(block) = state.blocks.get(&removed)
                            && !block.header.is_empty()
                        {
                            update.remove(&block.header);
                            any = true;
                        }
                        forget(state, removed);
                    }
                }
                Some(Op::Create) if is_code_block(data, span) && state.blocks.len() < MAX_BLOCKS => {
                    let (message, parent) = message_of(bytes, spans, span.id);
                    if message == 0 || parent == 0 {
                        continue;
                    }
                    let code = span_texts(data, span).unwrap_or_default();
                    let index = state.blocks.values().filter(|block| block.message == message).count() as u32;
                    state.blocks.insert(
                        span.id,
                        Block {
                            message,
                            index,
                            parent,
                            code,
                            dirty: true,
                            ..Block::default()
                        },
                    );
                    touched.push(span.id);
                }
                Some(Op::Update) if state.blocks.contains_key(&span.id) => {
                    if let Some(code) = span_texts(data, span)
                        && let Some(block) = state.blocks.get_mut(&span.id)
                    {
                        if block.card.is_some() && code.is_empty() {
                            continue;
                        }
                        block.code = code;
                        block.dirty = true;
                        touched.push(span.id);
                    }
                }
                Some(Op::SetChildren) => relisted.push(span),
                _ => {}
            }
        }
        for span in relisted {
            let tracked = state.parents.contains_key(&span.id) || state.blocks.values().any(|block| block.parent == span.id);
            if tracked {
                note_children(state, batch_of(bytes, span), span);
                if state.blocks.values().any(|block| block.parent == span.id && block.card.is_some()) {
                    put_parent(&mut update, state, span.id);
                    any = true;
                }
            }
        }
        for node in touched {
            let force = state.blocks.get_mut(&node).is_some_and(|block| std::mem::take(&mut block.dirty));
            any |= render(&mut update, state, node, force);
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
pub unsafe extern "C" fn astral_wire_refresh_code_blocks(buffer: *mut AstralWireBuffer) -> c_int {
    let Some(buffer) = (unsafe { buffer.as_mut() }) else { return 0 };
    *buffer = AstralWireBuffer {
        bytes: core::ptr::null_mut(),
        length: 0,
    };
    let mut out = Out::batch();
    let sent = with_state(|state| {
        let nodes: Vec<i32> = state.blocks.keys().copied().collect();
        nodes
            .into_iter()
            .fold(false, |sent, node| render(&mut out, state, node, false) | sent)
    });
    if !sent {
        out.discard();
        return 0;
    }
    c_int::from(out.finish(buffer))
}
