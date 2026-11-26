use super::tag::{Op, Tag};
use core::ffi::{c_char, c_int};
use std::collections::HashMap;

use super::abi::{
    AstralLabelContext, Callback, LABEL_AUTHOR, LABEL_CHANNEL, LABEL_COMPOSER, LABEL_MEMBER, LABEL_PROFILE, LABEL_REPLY, avatar_user,
    current_channel, prefixed, tracked_author,
};
use super::format::{Span, child, property_cursor, type_is};
use super::properties;

pub type Relabeler = unsafe extern "C" fn(*const AstralLabelContext, *const c_char, usize, *mut c_char, usize) -> c_int;

const MAX_EDITS: usize = 512;
const MAX_NODES: usize = 2048;
const MAX_LINKS: usize = 4096;
const LABEL_CAPACITY: usize = 256;
const PROFILE_NAME_SIZE: f64 = 20.0;
const ROW_DEPTH: usize = 24;
const LABEL_DEPTH: usize = 8;
const LABEL_ROWS: [(&core::ffi::CStr, c_int); 2] = [(c"navigation.channel:", LABEL_CHANNEL), (c"channel.details.member:", LABEL_MEMBER)];

static RELABELER: Callback = Callback::new();

#[unsafe(no_mangle)]
pub extern "C" fn astral_wire_set_relabeler(relabeler: Option<Relabeler>) {
    RELABELER.set(relabeler);
}

struct Edit {
    offset: usize,
    old_length: usize,
    text: Vec<u8>,
}

struct StringAt {
    offset: usize,
    length: usize,
}

impl StringAt {
    fn text<'a>(&self, bytes: &'a [u8]) -> &'a [u8] {
        &bytes[self.offset + 4..self.offset + 4 + self.length]
    }
}

fn string_at(bytes: &[u8], span: &Span, wanted: u16) -> Option<StringAt> {
    let (mut cursor, count) = property_cursor(bytes, span);
    for _ in 0..count {
        if cursor.failed() {
            break;
        }
        let property = cursor.read16();
        let tag = cursor.read8();
        if property == wanted && tag == Tag::String {
            let offset = cursor.offset();
            let length = cursor.read32() as usize;
            cursor.take(length)?;
            return Some(StringAt { offset, length });
        }
        cursor.skip_value(tag, 0);
    }
    None
}

fn number_at(bytes: &[u8], span: &Span, wanted: u16) -> Option<f64> {
    let (mut cursor, count) = property_cursor(bytes, span);
    for _ in 0..count {
        if cursor.failed() {
            break;
        }
        let property = cursor.read16();
        let tag = cursor.read8();
        if property == wanted && tag == Tag::Double {
            return Some(f64::from_le_bytes(cursor.take(8)?.try_into().ok()?));
        }
        cursor.skip_value(tag, 0);
    }
    None
}

fn first_span_text(bytes: &[u8], span: &Span) -> Option<StringAt> {
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
        if cursor.read32() == 0 || cursor.read8() != Tag::Map {
            return None;
        }
        let entries = cursor.read32();
        for _ in 0..entries {
            if cursor.failed() {
                break;
            }
            let key = cursor.read16();
            let value_tag = cursor.read8();
            if key == properties.text && value_tag == Tag::String {
                let offset = cursor.offset();
                let length = cursor.read32() as usize;
                cursor.take(length)?;
                return Some(StringAt { offset, length });
            }
            cursor.skip_value(value_tag, 0);
        }
        return None;
    }
    None
}

fn contains(bytes: &[u8], span: &Span, needle: &[u8]) -> bool {
    bytes
        .get(span.start..span.end)
        .is_some_and(|node| node.windows(needle.len()).any(|window| window == needle))
}

struct Relabel<'a> {
    bytes: &'a [u8],
    relabeler: Relabeler,
    edits: Vec<Edit>,
}

impl Relabel<'_> {
    fn add(&mut self, at: &StringAt, context: &AstralLabelContext) {
        if self.edits.len() >= MAX_EDITS || at.length >= LABEL_CAPACITY {
            return;
        }
        let current = at.text(self.bytes);
        let mut original = current.to_vec();
        original.push(0);
        let mut text = [0 as c_char; LABEL_CAPACITY];
        let written = unsafe { (self.relabeler)(context, original.as_ptr().cast(), current.len(), text.as_mut_ptr(), LABEL_CAPACITY) };
        if written < 0 || written as usize >= LABEL_CAPACITY {
            return;
        }
        let replacement = unsafe { core::slice::from_raw_parts(text.as_ptr().cast::<u8>(), written as usize) };
        if replacement == current {
            return;
        }
        self.edits.push(Edit {
            offset: at.offset,
            old_length: at.length,
            text: replacement.to_vec(),
        });
    }
}

fn label_context(kind: c_int, message: u64, channel: u64, target: u64) -> AstralLabelContext {
    AstralLabelContext {
        kind,
        message_id: message,
        channel_id: channel,
        target_id: target,
    }
}

fn label_profile(relabel: &mut Relabel, spans: &[Span]) {
    let properties = properties::current();
    let bytes = relabel.bytes;
    let mut profile = false;
    let mut user = 0;
    for span in spans.iter().filter(|span| span.op == Op::Create) {
        let data = &bytes[..span.end];
        if string_at(data, span, properties.accessibility_id).is_some_and(|at| at.text(data) == b"profile-scroll") {
            profile = true;
        }
        if user == 0
            && type_is(data, span, b"image")
            && let Some(uri) = string_at(data, span, properties.uri)
        {
            user = avatar_user(uri.text(data));
        }
    }
    if !profile {
        return;
    }
    for span in spans.iter().filter(|span| span.op == Op::Create) {
        let data = &bytes[..span.end];
        if !type_is(data, span, b"text") || number_at(data, span, properties.font_size).is_none_or(|size| size < PROFILE_NAME_SIZE) {
            continue;
        }
        let Some(text) = string_at(data, span, properties.text) else {
            continue;
        };
        relabel.add(&text, &label_context(LABEL_PROFILE, 0, current_channel(), user));
        return;
    }
}

struct Row {
    target: u64,
    kind: c_int,
    labelled: bool,
}

fn label_rows(relabel: &mut Relabel, spans: &[Span], links: &HashMap<i32, i32>) {
    label_profile(relabel, spans);
    let properties = properties::current();
    let bytes = relabel.bytes;
    let mut rows: HashMap<i32, Row> = HashMap::new();
    for span in spans.iter().filter(|span| span.op == Op::Create) {
        if rows.len() >= MAX_NODES / 4 {
            break;
        }
        let data = &bytes[..span.end];
        let Some(identifier) = string_at(data, span, properties.accessibility_id) else {
            continue;
        };
        let text = identifier.text(data);
        if text == b"composer" {
            if let Some(placeholder) = string_at(data, span, properties.placeholder) {
                relabel.add(&placeholder, &label_context(LABEL_COMPOSER, 0, current_channel(), 0));
            }
            continue;
        }
        if let Some((target, kind)) = LABEL_ROWS
            .iter()
            .find_map(|(prefix, kind)| prefixed(text, prefix).map(|target| (target, *kind)))
        {
            rows.insert(
                span.id,
                Row {
                    target,
                    kind,
                    labelled: false,
                },
            );
        }
    }
    if rows.is_empty() {
        return;
    }
    for span in spans.iter().filter(|span| span.op == Op::Create) {
        let data = &bytes[..span.end];
        if !type_is(data, span, b"text") {
            continue;
        }
        let Some(text) = string_at(data, span, properties.text) else {
            continue;
        };
        let mut node = span.id;
        for _ in 0..LABEL_DEPTH {
            node = links.get(&node).copied().unwrap_or(0);
            if node == 0 {
                break;
            }
            let Some(row) = rows.get_mut(&node) else { continue };
            if !row.labelled {
                row.labelled = true;
                let channel = if row.kind == LABEL_CHANNEL { row.target } else { current_channel() };
                let context = label_context(row.kind, 0, channel, row.target);
                relabel.add(&text, &context);
            }
            break;
        }
    }
}

fn row_of(links: &HashMap<i32, i32>, rows: &[i32], mut id: i32) -> i32 {
    for _ in 0..ROW_DEPTH {
        if id == 0 {
            break;
        }
        if rows.contains(&id) {
            return id;
        }
        id = links.get(&id).copied().unwrap_or(0);
    }
    0
}

fn shift(position: usize, edits: &[Edit], next: &mut usize, delta: &mut isize) -> usize {
    while *next < edits.len() && edits[*next].offset + 4 + edits[*next].old_length <= position {
        *delta += edits[*next].text.len() as isize - edits[*next].old_length as isize;
        *next += 1;
    }
    (position as isize + *delta) as usize
}

fn shift_spans(spans: &mut [Span], edits: &[Edit]) {
    let (mut starts, mut ends, mut children) = ((0, 0isize), (0, 0isize), (0, 0isize));
    for span in spans {
        span.start = shift(span.start, edits, &mut starts.0, &mut starts.1);
        span.end = shift(span.end, edits, &mut ends.0, &mut ends.1);
        if span.children != 0 {
            span.children = shift(span.children, edits, &mut children.0, &mut children.1);
        }
    }
}

fn collect(relabel: &mut Relabel, spans: &[Span]) {
    let properties = properties::current();
    let bytes = relabel.bytes;
    let mut links = HashMap::new();
    let mut rows = Vec::new();
    let mut authors: Vec<(i32, u64)> = Vec::new();
    for span in spans {
        let data = &bytes[..span.end];
        if span.op == Op::SetChildren {
            for position in 0..span.child_count {
                if links.len() < MAX_LINKS {
                    links.entry(child(data, span, position)).or_insert(span.id);
                }
            }
        } else if span.op == Op::Create
            && let Some(identifier) = string_at(data, span, properties.accessibility_id)
        {
            let text = identifier.text(data);
            if text == b"message-row" && rows.len() < MAX_NODES {
                rows.push(span.id);
            }
            if let Some(message) = prefixed(text, c"message.author:")
                && authors.len() < MAX_NODES
            {
                authors.push((span.id, message));
            }
        }
    }
    for span in spans.iter().filter(|span| span.op == Op::Create || span.op == Op::Update) {
        let data = &bytes[..span.end];
        let message = authors
            .iter()
            .rev()
            .find(|(node, _)| *node == span.id)
            .map(|(_, message)| *message)
            .or_else(|| tracked_author(span.id))
            .unwrap_or(0);
        if message != 0
            && let Some(text) = string_at(data, span, properties.text)
        {
            relabel.add(&text, &label_context(LABEL_AUTHOR, message, current_channel(), 0));
            continue;
        }
        if span.op != Op::Create || !contains(bytes, span, b"replyPreview") {
            continue;
        }
        let Some(text) = first_span_text(data, span) else { continue };
        let row = row_of(&links, &rows, span.id);
        if row == 0 {
            continue;
        }
        let replying = authors
            .iter()
            .rev()
            .find(|(node, _)| row_of(&links, &rows, *node) == row)
            .map(|(_, message)| *message)
            .unwrap_or(0);
        if replying != 0 {
            relabel.add(&text, &label_context(LABEL_REPLY, replying, current_channel(), 0));
        }
    }
    label_rows(relabel, spans, &links);
}

pub fn relabel(bytes: &[u8], spans: &mut [Span]) -> Option<Vec<u8>> {
    let relabeler = RELABELER.get::<Relabeler>()?;
    let mut relabel = Relabel {
        bytes,
        relabeler,
        edits: Vec::new(),
    };
    collect(&mut relabel, spans);
    let mut edits = relabel.edits;
    if edits.is_empty() {
        return None;
    }
    edits.sort_by_key(|edit| edit.offset);
    let mut result = Vec::with_capacity(bytes.len());
    let mut read = 0;
    for edit in &edits {
        result.extend_from_slice(&bytes[read..edit.offset]);
        result.extend_from_slice(&(edit.text.len() as u32).to_le_bytes());
        result.extend_from_slice(&edit.text);
        read = edit.offset + 4 + edit.old_length;
    }
    result.extend_from_slice(&bytes[read..]);
    shift_spans(spans, &edits);
    Some(result)
}

unsafe extern "C" {
    fn malloc(size: usize) -> *mut core::ffi::c_void;
}

#[unsafe(export_name = "wireRelabel")]
pub unsafe extern "C" fn wire_relabel(bytes: *const u8, length: usize, spans: *mut Span, count: usize, new_length: *mut usize) -> *mut u8 {
    if bytes.is_null() || spans.is_null() || new_length.is_null() {
        return core::ptr::null_mut();
    }
    let data = unsafe { core::slice::from_raw_parts(bytes, length) };
    let scanned = unsafe { core::slice::from_raw_parts_mut(spans, count) };
    let Some(result) = relabel(data, scanned) else {
        return core::ptr::null_mut();
    };
    let copy = unsafe { malloc(result.len().max(1)) }.cast::<u8>();
    if copy.is_null() {
        return core::ptr::null_mut();
    }
    unsafe {
        core::ptr::copy_nonoverlapping(result.as_ptr(), copy, result.len());
        *new_length = result.len();
    }
    copy
}
