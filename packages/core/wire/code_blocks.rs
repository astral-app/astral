use crate::lua_enums::TextStyle;
use core::ffi::{c_char, c_int};
use std::collections::HashMap;
use std::sync::Mutex;

use serde_json::Value;

use super::abi::Callback;
use super::decorate::wire_allocate_node;
use super::format::{Span, batch_of, child, find_string, property_cursor, spans_of, type_is, wire_creation, wire_parent_of};
use super::ids::current_channel;
use super::out::{AstralWireBuffer, Out};
use super::patch::wire_mark_refresh_busy;
use super::properties;
use crate::bridge::AstralRustString;

const CONTENT_PREFIX: &[u8] = b"message.content:";
const MONOSPACE: &[u8] = b"monospace";
const MAX_BLOCKS: usize = 256;
const MAX_ANCESTORS: usize = 4;
const HEADER_HEIGHT: f64 = 34.0;
const SIDE_PADDING: f32 = 12.0;
const ICON_SIZE: f64 = 16.0;
const CORNER: f64 = 6.0;
const BOLD: u64 = TextStyle::Bold.value() as u64;
const ITALIC: u64 = TextStyle::Italic.value() as u64;
const UNDERLINE: u64 = TextStyle::Underline.value() as u64;

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
        if property != properties.spans || tag != 7 {
            cursor.skip_value(tag, 0);
            continue;
        }
        let mut text = Vec::new();
        for _ in 0..cursor.read32() {
            if cursor.read8() != 8 {
                return None;
            }
            for _ in 0..cursor.read32() {
                let field = cursor.read16();
                let kind = cursor.read8();
                if field == properties.text && kind == 6 {
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
    span.op == 1
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

fn number(value: &Value, key: &str) -> Option<f64> {
    value.get(key).and_then(Value::as_f64)
}

fn color(value: &Value, key: &str) -> i64 {
    value.get(key).and_then(Value::as_i64).unwrap_or(0)
}

fn text<'a>(value: &'a Value, key: &str) -> &'a [u8] {
    value.get(key).and_then(Value::as_str).unwrap_or("").as_bytes()
}

fn put_long(out: &mut Out, property: u16, value: i64) {
    out.put16(property);
    out.put8(4);
    out.put(&value.to_le_bytes());
}

fn put_run(out: &mut Out, text: &[u8], color: i64, flags: u64) {
    let properties = properties::current();
    let fields = 2 + u32::from(flags & BOLD != 0) + u32::from(flags & ITALIC != 0) + u32::from(flags & UNDERLINE != 0);
    out.put8(8);
    out.put32(fields);
    out.prop_string(properties.text, text);
    put_long(out, properties.color, color);
    if flags & BOLD != 0 {
        out.prop_bool(properties.bold, true);
    }
    if flags & ITALIC != 0 {
        out.prop_bool(properties.italic, true);
    }
    if flags & UNDERLINE != 0 {
        out.prop_bool(properties.underline, true);
    }
}

fn line_runs(line: &Value, foreground: i64) -> Vec<(Vec<u8>, i64, u64)> {
    let runs: Vec<(Vec<u8>, i64, u64)> = line
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|run| {
            let text = run.get(0).and_then(Value::as_str).filter(|text| !text.is_empty())?;
            Some((
                text.as_bytes().to_vec(),
                run.get(1).and_then(Value::as_i64).unwrap_or(foreground),
                run.get(2).and_then(Value::as_u64).unwrap_or(0),
            ))
        })
        .collect();
    if runs.is_empty() {
        vec![(b" ".to_vec(), foreground, 0)]
    } else {
        runs
    }
}

fn put_code_text(out: &mut Out, node: i32, runs: &[(Vec<u8>, i64, u64)], card: &Value, grow: bool) {
    let properties = properties::current();
    out.create(node, b"text", if grow { 6 } else { 5 });
    out.put16(properties.spans);
    out.put8(7);
    out.put32(runs.len() as u32);
    for (text, color, flags) in runs {
        put_run(out, text, *color, *flags);
    }
    out.prop_string(properties.font_family, MONOSPACE);
    out.prop_number(properties.font_size, number(card, "fontSize").unwrap_or(12.0));
    out.prop_number(properties.line_height, number(card, "lineHeight").unwrap_or(14.5));
    put_long(out, properties.color, color(card, "foreground"));
    if grow {
        out.prop_number(properties.shrink, 1.0);
    }
}

fn put_body(out: &mut Out, block: &mut Block, card: &Value) -> i32 {
    let properties = properties::current();
    let foreground = color(card, "foreground");
    let numbers = color(card, "lineNumbers");
    let lines = card.get("lines").and_then(Value::as_array).cloned().unwrap_or_default();
    let width = lines.len().to_string().len().max(2);
    let body = wire_allocate_node();
    block.header.push(body);
    out.create(body, b"view", 6);
    out.prop_string(properties.direction, b"column");
    put_long(out, properties.background, color(card, "background"));
    out.prop_quad(properties.padding, [SIDE_PADDING, 8.0, SIDE_PADDING, 10.0]);
    out.prop_number(properties.radius_bottom_left, CORNER);
    out.prop_number(properties.radius_bottom_right, CORNER);
    out.prop_string(properties.width, b"fill");
    let mut rows = Vec::with_capacity(lines.len());
    for (index, line) in lines.iter().enumerate() {
        let row = wire_allocate_node();
        block.header.push(row);
        out.create(row, b"view", 2);
        out.prop_string(properties.direction, b"row");
        out.prop_string(properties.align, b"start");
        let mut cells = Vec::with_capacity(2);
        if numbers != 0 {
            let number_cell = wire_allocate_node();
            block.header.push(number_cell);
            put_code_text(
                out,
                number_cell,
                &[(format!("{:>width$} ", index + 1).into_bytes(), numbers, 0)],
                card,
                false,
            );
            cells.push(number_cell);
        }
        let code_cell = wire_allocate_node();
        block.header.push(code_cell);
        put_code_text(out, code_cell, &line_runs(line, foreground), card, true);
        cells.push(code_cell);
        out.put_children(row, &cells);
        rows.push(row);
    }
    out.put_children(body, &rows);
    body
}

fn put_card(out: &mut Out, block: &mut Block, card: &Value) {
    let properties = properties::current();
    if !block.header.is_empty() {
        out.remove(&block.header);
        block.header.clear();
    }
    let root = wire_allocate_node();
    block.header.push(root);
    out.create(root, b"view", 3);
    out.prop_string(properties.direction, b"column");
    out.prop_quad(properties.margin, [0.0, 8.0, 0.0, 0.0]);
    out.prop_string(properties.width, b"fill");
    let mut children = Vec::with_capacity(2);
    if let Some(header) = put_header(out, block, card) {
        children.push(header);
    }
    children.push(put_body(out, block, card));
    out.put_children(root, &children);
}

fn put_label(out: &mut Out, node: i32, label: &[u8], color: i64, weight: i32, grow: bool) {
    let properties = properties::current();
    out.create(node, b"text", if grow { 6 } else { 5 });
    out.prop_string(properties.text, label);
    put_long(out, properties.color, color);
    out.prop_number(properties.font_size, 11.0);
    out.prop_int(properties.font_weight, weight);
    out.prop_int(properties.max_lines, 1);
    if grow {
        out.prop_number(properties.flex, 1.0);
    }
}

fn put_header(out: &mut Out, block: &mut Block, card: &Value) -> Option<i32> {
    let properties = properties::current();
    let header = card.get("header").filter(|header| header.is_object())?;
    let root = wire_allocate_node();
    let foreground = color(header, "foreground");
    let tap = text(header, "tap");
    out.create(root, b"view", 7);
    out.prop_string(properties.direction, b"row");
    out.prop_string(properties.align, b"center");
    out.prop_number(properties.height, HEADER_HEIGHT);
    out.prop_quad(properties.padding, [SIDE_PADDING, 0.0, SIDE_PADDING, 0.0]);
    put_long(out, properties.background, color(header, "background"));
    out.prop_number(properties.radius_top_left, CORNER);
    out.prop_number(properties.radius_top_right, CORNER);
    block.header.push(root);
    let mut children = Vec::new();
    let uri = text(header, "icon");
    if !uri.is_empty() {
        let icon = wire_allocate_node();
        let tint = color(header, "iconTint");
        out.create(icon, b"image", if tint != 0 { 6 } else { 5 });
        out.prop_string(properties.uri, uri);
        out.prop_number(properties.width, ICON_SIZE);
        out.prop_number(properties.height, ICON_SIZE);
        out.prop_string(properties.scale, b"fit");
        out.prop_quad(properties.margin, [0.0, 0.0, 8.0, 0.0]);
        if tint != 0 {
            put_long(out, properties.tint, tint);
        }
        children.push(icon);
    }
    let label = wire_allocate_node();
    put_label(out, label, text(header, "label"), foreground, 600, true);
    children.push(label);
    if !tap.is_empty() {
        let copy = wire_allocate_node();
        put_label(out, copy, text(header, "copy"), foreground, 500, false);
        out.update(copy, 2);
        out.prop_string(properties.accessibility_id, tap);
        out.prop_events(properties.on, b"press");
        children.push(copy);
    }
    out.put_children(root, &children);
    block.header.extend(children);
    Some(root)
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
    unsafe { crate::bridge::astral_rust_string_free(card) };
    if outcome < 0 {
        wire_mark_refresh_busy();
        return block.card.clone();
    }
    if outcome == 0 {
        return None;
    }
    produced
        .and_then(|produced| serde_json::from_slice(&produced).ok())
        .filter(Value::is_object)
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
            match span.op {
                4 => {
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
                1 if is_code_block(data, span) && state.blocks.len() < MAX_BLOCKS => {
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
                2 if state.blocks.contains_key(&span.id) => {
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
                3 => relisted.push(span),
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
