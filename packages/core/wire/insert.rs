use super::tag::{Op, Tag};
use crate::lua_enums::{Align, Justify};
use core::ffi::{c_char, c_int};
use std::collections::HashMap;
use std::ffi::CString;
use std::sync::Mutex;

use super::abi::{AstralPatchTarget, Callback, current_channel, wire_message_author};
use super::children::{wire_discord_children, wire_discord_parent};
use super::decorate::{TRACK_AVATAR_IMAGE, tracked_value, wire_allocate_node, wire_write_host_layout};
use super::format::{Span, batch_of, child, find_flag, find_string, spans_of, wire_child_list, wire_creation, wire_parent_of};
use super::out::{AstralWireBuffer, Out};
use super::patch::{c_strings, message_of_identifier, read_identifier, wire_mark_refresh_busy};
use super::properties;

pub const INSERT_TEXT: c_int = 1;
pub const INSERT_IMAGE: c_int = 3;
pub const INSERT_PLAYER: c_int = 4;
pub const INSERT_CAPACITY: usize = 32;

const COLOR: u32 = 1 << 0;
const BACKGROUND: u32 = 1 << 1;
const RADIUS: u32 = 1 << 2;
const PADDING: u32 = 1 << 3;
const MARGIN: u32 = 1 << 4;
const FONT_SIZE: u32 = 1 << 5;
const WIDTH: u32 = 1 << 6;
const HEIGHT: u32 = 1 << 7;
const ROW: u32 = 1 << 8;
const TINT: u32 = 1 << 9;
const BOLD: u32 = 1 << 10;
const JUSTIFY: u32 = 1 << 11;
const FILL: u32 = 1 << 12;
const MUTED: u32 = 1 << 13;
const FILL_HEIGHT: u32 = 1 << 14;
const ALIGN: u32 = 1 << 15;

const IMAGE_SELECTOR: &[u8] = b"image:";

const MAX_PREFIXES: usize = 32;
const MAX_PREFIX_LENGTH: usize = 96;
const TARGET_CAPACITY: usize = 256;
const TARGET_CHILDREN: usize = 48;
const PATH_BASES: usize = 32;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AstralInsertNode {
    pub kind: c_int,
    pub parent: c_int,
    pub fields: u32,
    pub text: [c_char; 160],
    pub uri: [c_char; 256],
    pub tap: [c_char; 96],
    pub color: u32,
    pub background: u32,
    pub radius: f64,
    pub font_size: f64,
    pub font_weight: f64,
    pub width: f64,
    pub height: f64,
    pub padding: [f32; 4],
    pub margin: [f32; 4],
    pub justify: Justify,
    pub align: Align,
}

impl AstralInsertNode {
    fn empty() -> Self {
        AstralInsertNode {
            kind: 0,
            parent: 0,
            fields: 0,
            text: [0; 160],
            uri: [0; 256],
            tap: [0; 96],
            color: 0,
            background: 0,
            radius: 0.0,
            font_size: 0.0,
            font_weight: 0.0,
            width: 0.0,
            height: 0.0,
            padding: [0.0; 4],
            margin: [0.0; 4],
            justify: Justify::Start,
            align: Align::Start,
        }
    }
}

fn text_of(field: &[c_char]) -> &[u8] {
    let length = field.iter().position(|byte| *byte == 0).unwrap_or(field.len());
    unsafe { core::slice::from_raw_parts(field.as_ptr().cast::<u8>(), length) }
}

pub type NodeInserter = unsafe extern "C" fn(*const AstralPatchTarget, *mut AstralInsertNode, usize, *mut c_int) -> c_int;

#[derive(Clone, Default)]
struct Target {
    identifier: Vec<u8>,
    parent: Vec<u8>,
    label: Vec<u8>,
    checked: Option<bool>,
    kind: Vec<u8>,
    children: Vec<i32>,
    inserted: Vec<i32>,
    top_level: usize,
    at_start: bool,
    user: u64,
    built: u64,
}

#[derive(Clone)]
struct PathBase {
    node: i32,
    identifier: Vec<u8>,
    resolved: i32,
    user: u64,
}

#[derive(Default)]
struct State {
    prefixes: Vec<Vec<u8>>,
    targets: HashMap<i32, Target>,
    bases: Vec<PathBase>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
type Published = HashMap<i32, (Vec<i32>, bool)>;

static PUBLISHED: Mutex<Option<Published>> = Mutex::new(None);

fn publish(id: i32, target: &Target) {
    if let Ok(mut published) = PUBLISHED.lock() {
        published
            .get_or_insert_with(HashMap::new)
            .insert(id, (target.inserted[..target.top_level].to_vec(), target.at_start));
    }
}

fn unpublish(id: i32) {
    if let Ok(mut published) = PUBLISHED.lock()
        && let Some(published) = published.as_mut()
    {
        published.remove(&id);
    }
}
static INSERTER: Callback = Callback::new();

fn with_state<T>(work: impl FnOnce(&mut State) -> T) -> Option<T> {
    let mut state = STATE.lock().ok()?;
    Some(work(state.get_or_insert_with(State::default)))
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_wire_set_node_inserter(inserter: Option<NodeInserter>) {
    INSERTER.set(inserter);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_wire_set_insert_prefixes(prefixes: *const *const c_char, count: usize) {
    let prefixes = unsafe { c_strings(prefixes, count, MAX_PREFIXES, MAX_PREFIX_LENGTH) };
    with_state(|state| state.prefixes = prefixes);
}

fn prefix_length(prefixes: &[Vec<u8>], identifier: &[u8]) -> usize {
    prefixes.iter().find(|prefix| identifier.starts_with(prefix)).map_or(0, Vec::len)
}

fn batch_avatar_user(spans: &[Span]) -> u64 {
    spans
        .iter()
        .filter(|span| span.op == Op::Create)
        .find_map(|span| tracked_value(span.id, TRACK_AVATAR_IMAGE).filter(|user| *user != 0))
        .unwrap_or(0)
}

fn remember_children(target: &mut Target, children: impl Iterator<Item = i32>) {
    let inserted = target.inserted.clone();
    target.children = children.filter(|id| !inserted.contains(id)).take(TARGET_CHILDREN).collect();
}

fn list_children(bytes: *const u8, spans: &[Span], id: i32) -> Option<Vec<i32>> {
    let list = unsafe { wire_child_list(bytes, spans.as_ptr(), spans.len(), id).as_ref() }?;
    let data = batch_of(bytes, list);
    Some((0..list.child_count).map(|position| child(data, list, position)).collect())
}

fn known_children(id: i32) -> Vec<i32> {
    let mut known: *const i32 = core::ptr::null();
    let count = unsafe { wire_discord_children(id, &mut known) };
    if known.is_null() || count == 0 {
        Vec::new()
    } else {
        unsafe { core::slice::from_raw_parts(known, count as usize) }.to_vec()
    }
}

fn property_count(node: &AstralInsertNode) -> u16 {
    let mut count = node.fields.count_ones() as u16;
    count += u16::from(node.fields & ROW != 0);
    count -= u16::from(node.fields & ROW != 0 && node.fields & ALIGN != 0);
    count += u16::from(node.kind == INSERT_TEXT && !text_of(&node.text).is_empty());
    count += u16::from(node.kind == INSERT_IMAGE && !text_of(&node.uri).is_empty());
    count += 6 * u16::from(node.kind == INSERT_PLAYER && !text_of(&node.uri).is_empty());
    if !text_of(&node.tap).is_empty() {
        count += 2;
    }
    count
}

fn put_long(out: &mut Out, property: u16, value: u32) {
    out.put16(property);
    out.put_tag(Tag::Long);
    out.put(&i64::from(value).to_le_bytes());
}

fn put_node(out: &mut Out, id: i32, node: &AstralInsertNode) {
    let properties = properties::current();
    let kind: &[u8] = match node.kind {
        INSERT_TEXT => b"text",
        INSERT_IMAGE => b"image",
        INSERT_PLAYER => b"player",
        _ => b"view",
    };
    out.put_op(Op::Create);
    out.put(&id.to_le_bytes());
    out.put16(kind.len() as u16);
    out.put(kind);
    out.prop_count(property_count(node));
    let (text, uri, tap) = (text_of(&node.text), text_of(&node.uri), text_of(&node.tap));
    if node.kind == INSERT_TEXT && !text.is_empty() {
        out.prop_string(properties.text, text);
    }
    if node.kind == INSERT_IMAGE && !uri.is_empty() {
        out.prop_string(properties.uri, uri);
    }
    if node.kind == INSERT_PLAYER && !uri.is_empty() {
        out.prop_string(properties.uri, uri);
        out.prop_bool(properties.playing, true);
        out.prop_bool(properties.looping, true);
        out.prop_bool(properties.keep_awake, false);
        out.prop_bool(properties.start_after_scroll, false);
        out.prop_number(properties.visible_fraction, 0.0);
    }
    let has = |field: u32| node.fields & field != 0;
    if has(COLOR) {
        put_long(out, properties.color, node.color);
    }
    if has(TINT) {
        put_long(out, properties.tint, node.color);
    }
    if has(BACKGROUND) {
        put_long(out, properties.background, node.background);
    }
    if has(RADIUS) {
        out.prop_number(properties.radius, node.radius);
    }
    if has(PADDING) {
        out.prop_quad(properties.padding, node.padding);
    }
    if has(MARGIN) {
        out.prop_quad(properties.margin, node.margin);
    }
    if has(FONT_SIZE) {
        out.prop_number(properties.font_size, node.font_size);
    }
    if has(WIDTH) {
        out.prop_number(properties.width, node.width);
    }
    if has(HEIGHT) {
        out.prop_number(properties.height, node.height);
    }
    if has(BOLD) {
        out.prop_int(
            properties.font_weight,
            if node.font_weight > 0.0 { node.font_weight as i32 } else { 600 },
        );
    }
    if has(JUSTIFY) {
        out.prop_string(properties.justify, node.justify.value().as_bytes());
    }
    if has(FILL) {
        out.prop_string(properties.width, b"fill");
    }
    if has(FILL_HEIGHT) {
        out.prop_string(properties.height, b"fill");
    }
    if has(MUTED) {
        out.prop_bool(properties.muted, true);
    }
    if has(ROW) {
        out.prop_string(properties.direction, b"row");
        if !has(ALIGN) {
            out.prop_string(properties.align, Align::Center.value().as_bytes());
        }
    }
    if has(ALIGN) {
        out.prop_string(properties.align, node.align.value().as_bytes());
    }
    if !tap.is_empty() {
        out.prop_string(properties.accessibility_id, tap);
        out.put16(properties.on);
        out.put_tag(Tag::Array);
        out.put32(1);
        out.put_tag(Tag::String);
        out.put_string(b"press");
    }
}

fn put_children(out: &mut Out, parent: i32, children: &[i32]) {
    out.put_op(Op::SetChildren);
    out.put(&parent.to_le_bytes());
    out.put32(children.len() as u32);
    for id in children {
        out.put(&id.to_le_bytes());
    }
}

fn remove_inserted(out: &mut Out, target: &mut Target) {
    if target.inserted.is_empty() {
        return;
    }
    out.put_op(Op::Remove);
    out.put32(target.inserted.len() as u32);
    for id in &target.inserted {
        out.put(&id.to_le_bytes());
    }
    target.inserted.clear();
    target.top_level = 0;
}

#[unsafe(export_name = "wireInsertedChildren")]
pub unsafe extern "C" fn wire_inserted_children(parent: i32, children: *mut i32, capacity: u32, at_start: *mut c_int) -> u32 {
    let Ok(published) = PUBLISHED.lock() else { return 0 };
    let Some((top, start)) = published.as_ref().and_then(|published| published.get(&parent)) else {
        return 0;
    };
    let count = top.len().min(capacity as usize);
    if !children.is_null() {
        unsafe { core::ptr::copy_nonoverlapping(top.as_ptr(), children, count) };
    }
    if !at_start.is_null() {
        unsafe { *at_start = c_int::from(*start) };
    }
    count as u32
}

fn put_target_children(out: &mut Out, id: i32, target: &Target) {
    publish(id, target);
    if unsafe { wire_write_host_layout(out, id) } != 0 {
        return;
    }
    let top = &target.inserted[..target.top_level];
    let children: Vec<i32> = if target.at_start {
        top.iter().chain(&target.children).copied().collect()
    } else {
        target.children.iter().chain(top).copied().collect()
    };
    put_children(out, id, &children);
}

fn build_nodes(out: &mut Out, target: &mut Target, nodes: &[AstralInsertNode]) {
    let ids: Vec<i32> = nodes.iter().map(|_| wire_allocate_node()).collect();
    for (id, node) in ids.iter().zip(nodes) {
        put_node(out, *id, node);
    }
    let top: Vec<i32> = nodes
        .iter()
        .zip(&ids)
        .enumerate()
        .filter(|(index, (node, _))| !(node.parent >= 0 && (node.parent as usize) < *index))
        .map(|(_, (_, id))| *id)
        .collect();
    for parent in 0..nodes.len() {
        let children: Vec<i32> = ((parent + 1)..nodes.len())
            .filter(|index| nodes[*index].parent == parent as c_int)
            .map(|index| ids[index])
            .collect();
        if !children.is_empty() {
            put_children(out, ids[parent], &children);
        }
    }
    target.top_level = top.len();
    target.inserted = top;
    for id in ids {
        if !target.inserted.contains(&id) && target.inserted.len() < INSERT_CAPACITY {
            target.inserted.push(id);
        }
    }
}

fn built_signature(nodes: &[AstralInsertNode], at_start: c_int) -> u64 {
    let bytes = unsafe { core::slice::from_raw_parts(nodes.as_ptr().cast::<u8>(), core::mem::size_of_val(nodes)) };
    bytes
        .iter()
        .chain(&at_start.to_le_bytes())
        .fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
        })
}

fn evaluate(out: &mut Out, prefixes: &[Vec<u8>], id: i32, target: &mut Target, keep_unchanged: bool) -> bool {
    let Some(inserter) = INSERTER.get::<NodeInserter>() else {
        return false;
    };
    let (Ok(identifier), Ok(kind)) = (CString::new(target.identifier.clone()), CString::new(target.kind.clone())) else {
        return false;
    };
    let (Ok(parent), Ok(label)) = (CString::new(target.parent.clone()), CString::new(target.label.clone())) else {
        return false;
    };
    let message = message_of_identifier(&target.identifier);
    let context = AstralPatchTarget {
        identifier: identifier.as_ptr(),
        kind: kind.as_ptr(),
        key: prefix_length(prefixes, &target.identifier),
        message_id: message,
        author_id: if message != 0 { wire_message_author(message) } else { target.user },
        channel_id: current_channel(),
        parent: if target.parent.is_empty() {
            core::ptr::null()
        } else {
            parent.as_ptr()
        },
        node: id,
        checked: target.checked.map_or(-1, c_int::from),
        label: if target.label.is_empty() {
            core::ptr::null()
        } else {
            label.as_ptr()
        },
    };
    let mut nodes = vec![AstralInsertNode::empty(); INSERT_CAPACITY];
    let mut at_start = 0;
    let count = unsafe { inserter(&context, nodes.as_mut_ptr(), INSERT_CAPACITY, &mut at_start) };
    if count < 0 {
        wire_mark_refresh_busy();
        return false;
    }
    if count == 0 && target.inserted.is_empty() {
        return false;
    }
    let built = &nodes[..(count as usize).min(INSERT_CAPACITY)];
    let signature = built_signature(built, at_start);
    if keep_unchanged && count > 0 && !target.inserted.is_empty() && signature == target.built {
        return false;
    }
    target.built = signature;
    remove_inserted(out, target);
    target.at_start = at_start != 0;
    if count > 0 {
        build_nodes(out, target, built);
    }
    put_target_children(out, id, target);
    true
}

fn path_steps<'a>(prefix: &'a [u8], identifier: &[u8]) -> Option<&'a [u8]> {
    let steps = prefix.strip_prefix(identifier)?;
    (!steps.is_empty() && steps.iter().all(|step| *step == b'>' || *step == b'^')).then_some(steps)
}

fn follow_path(bytes: *const u8, spans: &[Span], mut node: i32, steps: &[u8]) -> i32 {
    for step in steps {
        if node <= 0 {
            break;
        }
        node = if *step == b'^' {
            let parent = unsafe { wire_parent_of(bytes, spans.as_ptr(), spans.len(), node) };
            if parent > 0 { parent } else { wire_discord_parent(node) }
        } else {
            list_children(bytes, spans, node)
                .and_then(|children| children.first().copied())
                .or_else(|| known_children(node).first().copied())
                .unwrap_or(0)
        };
    }
    node
}

struct Writer<'a> {
    out: &'a mut Out,
    changed: &'a mut c_int,
    original: &'a [u8],
}

impl Writer<'_> {
    fn append(&mut self, staged: &Out) {
        if *self.changed == 0 {
            self.out.put(self.original);
        }
        *self.changed = 1;
        self.out.put(unsafe { core::slice::from_raw_parts(staged.bytes, staged.length) });
    }
}

fn insert_at_paths(state: &mut State, bytes: *const u8, spans: &[Span], base: &mut PathBase, writer: &mut Writer) {
    let prefixes = state.prefixes.clone();
    for prefix in &prefixes {
        let Some(steps) = path_steps(prefix, &base.identifier) else {
            continue;
        };
        let resolved = follow_path(bytes, spans, base.node, steps);
        if resolved <= 0 {
            continue;
        }
        base.resolved = resolved;
        if state.targets.contains_key(&resolved) || state.targets.len() >= TARGET_CAPACITY {
            continue;
        }
        let mut target = Target {
            identifier: prefix.clone(),
            kind: b"view".to_vec(),
            user: if base.user != 0 { base.user } else { batch_avatar_user(spans) },
            ..Target::default()
        };
        if let Some(creation) = unsafe { wire_creation(spans.as_ptr(), spans.len(), resolved).as_ref() } {
            remember_state(&mut target, batch_of(bytes, creation), creation);
        }
        match list_children(bytes, spans, resolved) {
            Some(children) => remember_children(&mut target, children.into_iter()),
            None => target.children = known_children(resolved).into_iter().take(TARGET_CHILDREN).collect(),
        }
        let mut staged = Out::default();
        if evaluate(&mut staged, &prefixes, resolved, &mut target, false) {
            writer.append(&staged);
        }
        staged.discard();
        state.targets.insert(resolved, target);
    }
}

fn remember_state(target: &mut Target, data: &[u8], span: &Span) {
    let properties = properties::current();
    target.checked = find_flag(data, span, properties.accessibility_checked);
    target.label = find_string(data, span, properties.accessibility_label)
        .map(<[u8]>::to_vec)
        .unwrap_or_default();
}

fn image_selector(prefixes: &[Vec<u8>], uri: &[u8]) -> bool {
    prefixes
        .iter()
        .filter_map(|prefix| prefix.strip_prefix(IMAGE_SELECTOR))
        .any(|selector| !selector.is_empty() && uri.starts_with(selector))
}

fn insert_at_image(state: &mut State, bytes: *const u8, spans: &[Span], span: &Span, writer: &mut Writer) {
    let data = batch_of(bytes, span);
    let properties = properties::current();
    let Some(uri) = find_string(data, span, properties.uri) else {
        return;
    };
    if !image_selector(&state.prefixes, uri) {
        return;
    }
    let parent = unsafe { wire_parent_of(bytes, spans.as_ptr(), spans.len(), span.id) };
    let parent = if parent > 0 { parent } else { wire_discord_parent(span.id) };
    if parent <= 0 || state.targets.contains_key(&parent) || state.targets.len() >= TARGET_CAPACITY {
        return;
    }
    let creation = unsafe { wire_creation(spans.as_ptr(), spans.len(), parent).as_ref() };
    let parent_identifier = creation
        .and_then(|creation| read_identifier(batch_of(bytes, creation), creation))
        .map(|(identifier, _)| identifier.to_vec())
        .unwrap_or_default();
    let mut identifier = IMAGE_SELECTOR.to_vec();
    identifier.extend_from_slice(uri);
    let mut target = Target {
        identifier,
        parent: parent_identifier,
        kind: b"image".to_vec(),
        ..Target::default()
    };
    if let Some(children) = list_children(bytes, spans, parent) {
        remember_children(&mut target, children.into_iter());
    }
    let prefixes = state.prefixes.clone();
    let mut staged = Out::default();
    if evaluate(&mut staged, &prefixes, parent, &mut target, false) {
        writer.append(&staged);
    }
    staged.discard();
    state.targets.insert(parent, target);
}

fn note_bases(state: &mut State, bytes: *const u8, spans: &[Span]) {
    for span in spans {
        let data = batch_of(bytes, span);
        if span.op == Op::Remove {
            let removed: Vec<i32> = (0..span.child_count).map(|position| child(data, span, position)).collect();
            state.bases.retain(|base| !removed.contains(&base.node));
            continue;
        }
        if span.op != Op::Create {
            continue;
        }
        let Some((identifier, _)) = read_identifier(data, span) else {
            continue;
        };
        if !state.prefixes.iter().any(|prefix| path_steps(prefix, identifier).is_some()) {
            continue;
        }
        let base = PathBase {
            node: span.id,
            identifier: identifier.to_vec(),
            resolved: 0,
            user: batch_avatar_user(spans),
        };
        match state.bases.iter().position(|existing| existing.node == span.id) {
            Some(index) => state.bases[index] = base,
            None if state.bases.len() < PATH_BASES => state.bases.push(base),
            None => state.bases[span.id.unsigned_abs() as usize % PATH_BASES] = base,
        }
    }
}

#[unsafe(export_name = "wireInsertNodes")]
pub unsafe extern "C" fn wire_insert_nodes(
    bytes: *const u8,
    pointer: *const Span,
    count: usize,
    out: *mut Out,
    changed: *mut c_int,
    length: usize,
) {
    if INSERTER.get::<NodeInserter>().is_none() {
        return;
    }
    let (Some(out), Some(changed)) = (unsafe { out.as_mut() }, unsafe { changed.as_mut() }) else {
        return;
    };
    let spans = spans_of(pointer, count);
    let mut writer = Writer {
        out,
        changed,
        original: unsafe { core::slice::from_raw_parts(bytes, length) },
    };
    with_state(|state| {
        if state.prefixes.is_empty() {
            return;
        }
        let prefixes = state.prefixes.clone();
        for span in spans {
            let data = batch_of(bytes, span);
            match Op::parse(span.op) {
                Some(Op::Remove) => {
                    for position in 0..span.child_count {
                        let removed = child(data, span, position);
                        state.targets.remove(&removed);
                        unpublish(removed);
                    }
                }
                Some(Op::SetChildren) => {
                    let created = !unsafe { wire_creation(spans.as_ptr(), spans.len(), span.id) }.is_null();
                    let Some(target) = state.targets.get_mut(&span.id).filter(|_| !created) else {
                        continue;
                    };
                    remember_children(target, (0..span.child_count).map(|position| child(data, span, position)));
                    if target.top_level == 0 {
                        continue;
                    }
                    if *writer.changed == 0 {
                        writer.out.put(writer.original);
                    }
                    *writer.changed = 1;
                    put_target_children(writer.out, span.id, target);
                }
                Some(Op::Create) => {
                    let Some((identifier, kind)) = read_identifier(data, span) else {
                        insert_at_image(state, bytes, spans, span, &mut writer);
                        continue;
                    };
                    if prefix_length(&prefixes, identifier) == 0 {
                        continue;
                    }
                    if state.targets.len() >= TARGET_CAPACITY && !state.targets.contains_key(&span.id) {
                        continue;
                    }
                    let mut target = state.targets.remove(&span.id).unwrap_or_default();
                    target.identifier = identifier.to_vec();
                    target.kind = kind.to_vec();
                    remember_state(&mut target, data, span);
                    if let Some(children) = list_children(bytes, spans, span.id) {
                        remember_children(&mut target, children.into_iter());
                    }
                    let mut staged = Out::default();
                    if evaluate(&mut staged, &prefixes, span.id, &mut target, false) {
                        writer.append(&staged);
                    }
                    staged.discard();
                    state.targets.insert(span.id, target);
                }
                _ => {}
            }
        }
        note_bases(state, bytes, spans);
        let mut bases = core::mem::take(&mut state.bases);
        for base in bases.iter_mut() {
            if base.resolved != 0 && state.targets.contains_key(&base.resolved) {
                continue;
            }
            insert_at_paths(state, bytes, spans, base, &mut writer);
        }
        state.bases = bases;
    });
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_wire_refresh_node_inserts(buffer: *mut AstralWireBuffer) -> c_int {
    let Some(buffer) = (unsafe { buffer.as_mut() }) else { return 0 };
    *buffer = AstralWireBuffer {
        bytes: core::ptr::null_mut(),
        length: 0,
    };
    if INSERTER.get::<NodeInserter>().is_none() {
        return 0;
    }
    let mut out = Out::batch();
    let any = with_state(|state| {
        let prefixes = state.prefixes.clone();
        let mut any = false;
        for (id, target) in state.targets.iter_mut() {
            any |= evaluate(&mut out, &prefixes, *id, target, true);
        }
        any
    })
    .unwrap_or(false);
    if !any {
        out.discard();
        return 0;
    }
    c_int::from(out.finish(buffer))
}
