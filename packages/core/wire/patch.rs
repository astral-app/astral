use super::tag::{Op, Tag};
use core::ffi::{c_char, c_int};
use std::collections::HashMap;
use std::ffi::CString;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use super::abi::{
    AstralNodePatch, AstralPatchTarget, Callback, PATCH_BACKGROUND, PATCH_BORDER, PATCH_GRADIENT, PATCH_HIDDEN, PATCH_MARGIN,
    PATCH_OPACITY, PATCH_PADDING, PATCH_RADIUS, PATCH_SEE_THROUGH, PATCH_SELECTABLE, PATCH_TEXT_COLOR, PATCH_TEXT_SCALE, current_channel,
    wire_message_author, wire_row_message,
};
use super::cursor::Cursor;
use super::format::{Span, batch_of, child, spans_of, type_is, wire_child_list, wire_creation};
use super::ids::parse_id;
use super::out::{AstralWireBuffer, Out};
use super::properties::{self, AstralWireProperties};

pub type NodePatcher = unsafe extern "C" fn(*const AstralPatchTarget, *mut AstralNodePatch) -> c_int;

const MAX_PREFIXES: usize = 32;
const MAX_PREFIX_LENGTH: usize = 96;
const PATCHED_CAPACITY: usize = 512;
const RAW_CAPACITY: usize = 40;
const PATCH_PROPS: usize = Slot::ALL.len();

#[derive(Clone, Copy, PartialEq, Eq)]
enum Slot {
    Background,
    Radius,
    Opacity,
    Padding,
    Margin,
    BorderColor,
    BorderWidth,
    TextColor,
    Width,
    Height,
    Clip,
    Gradient,
}

impl Slot {
    const ALL: [Slot; 12] = [
        Slot::Background,
        Slot::Radius,
        Slot::Opacity,
        Slot::Padding,
        Slot::Margin,
        Slot::BorderColor,
        Slot::BorderWidth,
        Slot::TextColor,
        Slot::Width,
        Slot::Height,
        Slot::Clip,
        Slot::Gradient,
    ];

    const fn index(self) -> usize {
        self as usize
    }

    const fn bit(self) -> u32 {
        1 << self as u32
    }

    const fn property(self, properties: &AstralWireProperties) -> u16 {
        match self {
            Slot::Background => properties.background,
            Slot::Radius => properties.radius,
            Slot::Opacity => properties.opacity,
            Slot::Padding => properties.padding,
            Slot::Margin => properties.margin,
            Slot::BorderColor => properties.border_color,
            Slot::BorderWidth => properties.border_width,
            Slot::TextColor => properties.color,
            Slot::Width => properties.width,
            Slot::Height => properties.height,
            Slot::Clip => properties.clip,
            Slot::Gradient => properties.background_gradient,
        }
    }
}
const SCALED_CAPACITY: usize = 2048;
const SCALE_DEPTH: u32 = 8;
const SCALE_NODES: usize = 256;
pub const IDENTIFIER_LENGTH: usize = 128;
pub const TYPE_LENGTH: usize = 16;

#[derive(Clone, Default)]
struct RawProp {
    tag: u8,
    value: Vec<u8>,
}

#[derive(Clone, Default)]
struct Patched {
    identifier: Vec<u8>,
    kind: Vec<u8>,
    applied: u32,
    originals: [Option<RawProp>; PATCH_PROPS],
    see_through: Vec<i32>,
    background: u32,
    cleared: bool,
    written: Option<AstralNodePatch>,
}

struct ScaledText {
    owner: i32,
    base: f64,
    scaled: bool,
    scale: f64,
    selectable: bool,
}

#[derive(Default)]
struct State {
    prefixes: Vec<Vec<u8>>,
    patched: HashMap<i32, Patched>,
    scaled: HashMap<i32, ScaledText>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static PATCHER: Callback = Callback::new();
static REFRESH_BUSY: AtomicBool = AtomicBool::new(false);

fn with_state<T>(work: impl FnOnce(&mut State) -> T) -> Option<T> {
    let mut state = STATE.lock().ok()?;
    Some(work(state.get_or_insert_with(State::default)))
}

#[unsafe(export_name = "wireMarkRefreshBusy")]
pub extern "C" fn wire_mark_refresh_busy() {
    REFRESH_BUSY.store(true, Ordering::Relaxed);
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_wire_take_refresh_busy() -> c_int {
    c_int::from(REFRESH_BUSY.swap(false, Ordering::Relaxed))
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_wire_set_node_patcher(patcher: Option<NodePatcher>) {
    PATCHER.set(patcher);
}

pub unsafe fn c_strings(list: *const *const c_char, count: usize, limit: usize, max_length: usize) -> Vec<Vec<u8>> {
    if list.is_null() {
        return Vec::new();
    }
    unsafe { core::slice::from_raw_parts(list, count) }
        .iter()
        .filter(|text| !text.is_null())
        .map(|text| unsafe { core::ffi::CStr::from_ptr(*text) }.to_bytes().to_vec())
        .filter(|text| !text.is_empty() && text.len() < max_length)
        .take(limit)
        .collect()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_wire_set_patch_prefixes(prefixes: *const *const c_char, count: usize) {
    let prefixes = unsafe { c_strings(prefixes, count, MAX_PREFIXES, MAX_PREFIX_LENGTH) };
    with_state(|state| state.prefixes = prefixes);
}

#[unsafe(export_name = "wirePatchingActive")]
pub extern "C" fn wire_patching_active() -> c_int {
    let prefixes = with_state(|state| !state.prefixes.is_empty()).unwrap_or(false);
    c_int::from(PATCHER.get::<NodePatcher>().is_some() && prefixes)
}

pub fn message_of_identifier(identifier: &[u8]) -> u64 {
    if !identifier.starts_with(b"message.") {
        return 0;
    }
    identifier
        .iter()
        .position(|byte| *byte == b':')
        .map_or(0, |colon| parse_id(&identifier[colon + 1..]))
}

#[unsafe(export_name = "wireMessageOfIdentifier")]
pub unsafe extern "C" fn wire_message_of_identifier(identifier: *const c_char) -> u64 {
    if identifier.is_null() {
        return 0;
    }
    message_of_identifier(unsafe { core::ffi::CStr::from_ptr(identifier) }.to_bytes())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_wire_message_of(identifier: *const c_char) -> u64 {
    unsafe { wire_message_of_identifier(identifier) }
}

pub fn read_identifier<'a>(bytes: &'a [u8], span: &Span) -> Option<(&'a [u8], &'a [u8])> {
    let accessibility = properties::current().accessibility_id;
    let mut cursor = Cursor::new(bytes, span.start + 5);
    let type_length = cursor.read16() as usize;
    if type_length >= TYPE_LENGTH {
        return None;
    }
    let kind = cursor.take(type_length)?;
    let count = cursor.read16();
    for _ in 0..count {
        if cursor.failed() {
            break;
        }
        let property = cursor.read16();
        let tag = cursor.read8();
        if property == accessibility && tag == Tag::String {
            let length = cursor.read32() as usize;
            if length >= IDENTIFIER_LENGTH {
                return None;
            }
            return Some((cursor.take(length)?, kind));
        }
        cursor.skip_value(tag, 0);
    }
    None
}

fn copy_c(text: &[u8], out: *mut c_char, capacity: usize) {
    let length = text.len().min(capacity - 1);
    unsafe {
        core::ptr::copy_nonoverlapping(text.as_ptr(), out.cast::<u8>(), length);
        *out.add(length) = 0;
    }
}

#[unsafe(export_name = "wireReadIdentifier")]
pub unsafe extern "C" fn wire_read_identifier(bytes: *const u8, span: *const Span, identifier: *mut c_char, kind: *mut c_char) -> c_int {
    let Some(span) = (unsafe { span.as_ref() }) else { return 0 };
    if identifier.is_null() || kind.is_null() {
        return 0;
    }
    let Some((text, name)) = read_identifier(batch_of(bytes, span), span) else {
        return 0;
    };
    copy_c(text, identifier, IDENTIFIER_LENGTH);
    copy_c(name, kind, TYPE_LENGTH);
    1
}

fn key_offset(prefixes: &[Vec<u8>], identifier: &[u8]) -> Option<usize> {
    prefixes.iter().find(|prefix| identifier.starts_with(prefix)).map(Vec::len)
}

pub fn patch_target(prefix_length: usize, identifier: &CString, kind: &CString, node: i32) -> AstralPatchTarget {
    let mut message = message_of_identifier(identifier.as_bytes());
    if message == 0 && identifier.as_bytes() == b"message-row" {
        message = wire_row_message(node);
    }
    AstralPatchTarget {
        identifier: identifier.as_ptr(),
        kind: kind.as_ptr(),
        key: prefix_length,
        message_id: message,
        author_id: if message != 0 { wire_message_author(message) } else { 0 },
        channel_id: current_channel(),
        parent: core::ptr::null(),
        node,
        checked: -1,
        label: core::ptr::null(),
    }
}

fn call_patcher(prefix_length: usize, node: i32, identifier: &[u8], kind: &[u8], patch: &mut AstralNodePatch) -> c_int {
    let Some(patcher) = PATCHER.get::<NodePatcher>() else { return 0 };
    let (Ok(identifier), Ok(kind)) = (CString::new(identifier), CString::new(kind)) else {
        return 0;
    };
    let target = patch_target(prefix_length, &identifier, &kind, node);
    unsafe { patcher(&target, patch) }
}

fn capture_originals(bytes: &[u8], span: &Span, patched: &mut Patched, properties: &AstralWireProperties) {
    let mut cursor = Cursor::new(bytes, span.start + 5);
    let type_length = cursor.read16() as usize;
    cursor.skip(type_length);
    let count = cursor.read16();
    for _ in 0..count {
        if cursor.failed() {
            break;
        }
        let property = cursor.read16();
        let tag = cursor.read8();
        let start = cursor.offset();
        cursor.skip_value(tag, 0);
        let Some(value) = bytes.get(start..cursor.offset()) else { break };
        for slot in Slot::ALL {
            if slot.property(properties) == property && value.len() <= RAW_CAPACITY {
                patched.originals[slot.index()] = Some(RawProp {
                    tag,
                    value: value.to_vec(),
                });
            }
        }
    }
}

fn patch_fields(patch: &AstralNodePatch) -> u32 {
    if patch.fields & PATCH_HIDDEN != 0 {
        patch.fields | PATCH_OPACITY | PATCH_PADDING | PATCH_MARGIN
    } else {
        patch.fields
    }
}

fn slot_mask(fields: u32) -> u32 {
    [
        (PATCH_BACKGROUND, Slot::Background.bit()),
        (PATCH_RADIUS, Slot::Radius.bit()),
        (PATCH_OPACITY, Slot::Opacity.bit()),
        (PATCH_PADDING, Slot::Padding.bit()),
        (PATCH_MARGIN, Slot::Margin.bit()),
        (PATCH_BORDER, Slot::BorderColor.bit() | Slot::BorderWidth.bit()),
        (PATCH_TEXT_COLOR, Slot::TextColor.bit()),
        (PATCH_HIDDEN, Slot::Width.bit() | Slot::Height.bit() | Slot::Clip.bit()),
        (PATCH_GRADIENT, Slot::Gradient.bit()),
    ]
    .iter()
    .filter(|(field, _)| fields & field != 0)
    .fold(0, |mask, (_, bits)| mask | bits)
}

fn put_long(out: &mut Out, property: u16, value: u32) {
    out.put16(property);
    out.put_tag(Tag::Long);
    out.put(&i64::from(value).to_le_bytes());
}

fn put_patched(out: &mut Out, slot: Slot, patch: &AstralNodePatch, properties: &AstralWireProperties) {
    let hidden = patch.fields & PATCH_HIDDEN != 0;
    let property = slot.property(properties);
    match slot {
        Slot::Background => put_long(out, property, patch.background),
        Slot::Radius => out.prop_number(property, patch.radius),
        Slot::Opacity => out.prop_number(property, if hidden { 0.0 } else { patch.opacity }),
        Slot::Padding => out.prop_quad(property, if hidden { [0.0; 4] } else { patch.padding }),
        Slot::Margin => out.prop_quad(property, if hidden { [0.0; 4] } else { patch.margin }),
        Slot::BorderColor => put_long(out, property, patch.border_color),
        Slot::BorderWidth => out.prop_number(property, patch.border_width),
        Slot::TextColor => put_long(out, property, patch.text_color),
        Slot::Width | Slot::Height => out.prop_number(property, 0.0),
        Slot::Clip => {
            out.put16(property);
            out.put_tag(Tag::True);
        }
        Slot::Gradient => unsafe {
            super::out::wire_prop_gradient(
                out,
                property,
                patch.gradient_angle,
                patch.gradient_stops.as_ptr(),
                patch.gradient_count.min(patch.gradient_stops.len() as u32),
            )
        },
    }
}

fn put_original(out: &mut Out, slot: Slot, raw: Option<&RawProp>, properties: &AstralWireProperties) {
    out.put16(slot.property(properties));
    match raw {
        Some(raw) => {
            out.put8(raw.tag);
            out.put(&raw.value);
        }
        None => out.put_tag(Tag::Null),
    }
}

fn write_patch(out: &mut Out, id: i32, patched: &mut Patched, patch: &AstralNodePatch) -> bool {
    if patched.written.as_ref() == Some(patch) {
        return false;
    }
    patched.written = Some(*patch);
    let properties = properties::current();
    let wanted = slot_mask(patch_fields(patch));
    let touched = wanted | patched.applied;
    if touched == 0 {
        return false;
    }
    out.update(id, touched.count_ones() as u16);
    for slot in Slot::ALL.into_iter().filter(|slot| touched & slot.bit() != 0) {
        if wanted & slot.bit() != 0 {
            put_patched(out, slot, patch, &properties);
        } else {
            put_original(out, slot, patched.originals[slot.index()].as_ref(), &properties);
        }
    }
    patched.applied = wanted;
    write_see_through(out, patched, patch.fields & PATCH_SEE_THROUGH != 0);
    true
}

fn write_see_through(out: &mut Out, patched: &mut Patched, wanted: bool) {
    if wanted == patched.cleared || patched.see_through.is_empty() {
        return;
    }
    let background = properties::current().background;
    let color = if wanted { 0 } else { patched.background };
    for id in &patched.see_through {
        out.update(*id, 1);
        put_long(out, background, color);
    }
    patched.cleared = wanted;
}

const SEE_THROUGH_DEPTH: u32 = 4;
const SEE_THROUGH_NODES: usize = 64;

fn collect_see_through(bytes: *const u8, spans: &[Span], id: i32, background: u32, depth: u32, found: &mut Vec<i32>) {
    if depth > SEE_THROUGH_DEPTH || found.len() >= SEE_THROUGH_NODES {
        return;
    }
    let Some(list) = (unsafe { wire_child_list(bytes, spans.as_ptr(), spans.len(), id).as_ref() }) else {
        return;
    };
    let background_property = properties::current().background;
    let data = batch_of(bytes, list);
    for position in 0..list.child_count {
        if found.len() >= SEE_THROUGH_NODES {
            break;
        }
        let child_id = child(data, list, position);
        let creation = unsafe { wire_creation(spans.as_ptr(), spans.len(), child_id) };
        let mut color = 0;
        if !creation.is_null()
            && unsafe { super::format::wire_color_property(bytes, creation, background_property, &mut color) } != 0
            && color == background
        {
            found.push(child_id);
        }
        collect_see_through(bytes, spans, child_id, background, depth + 1, found);
    }
}

fn write_scaled(out: &mut Out, scaled: &mut HashMap<i32, ScaledText>, owner: i32, patch: &AstralNodePatch) -> bool {
    let properties = properties::current();
    let scale = if patch.fields & PATCH_TEXT_SCALE != 0 {
        patch.text_scale
    } else {
        1.0
    };
    let selectable = patch.fields & PATCH_SELECTABLE != 0;
    let mut wrote = false;
    for (id, text) in scaled.iter_mut().filter(|(_, text)| text.owner == owner) {
        let rescale = !((scale == 1.0 && !text.scaled) || (text.scaled && text.scale == scale));
        let reselect = text.selectable != selectable;
        if !rescale && !reselect {
            continue;
        }
        out.update(*id, u16::from(rescale) + u16::from(reselect));
        if rescale {
            out.prop_number(properties.font_size, text.base * scale);
            text.scaled = scale != 1.0;
            text.scale = scale;
        }
        if reselect {
            out.prop_bool(properties.selectable, selectable);
            text.selectable = selectable;
        }
        wrote = true;
    }
    wrote
}

fn font_size(bytes: &[u8], span: &Span) -> Option<f64> {
    let wanted = properties::current().font_size;
    let mut cursor = Cursor::new(bytes, span.start + 5);
    let type_length = cursor.read16() as usize;
    cursor.skip(type_length);
    let count = cursor.read16();
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

fn collect_texts(scaled: &mut HashMap<i32, ScaledText>, bytes: *const u8, spans: &[Span], owner: i32) {
    let mut queue = vec![(owner, 0u32)];
    let mut cursor = 0;
    while cursor < queue.len() {
        let (node, depth) = queue[cursor];
        cursor += 1;
        if let Some(created) = unsafe { wire_creation(spans.as_ptr(), spans.len(), node).as_ref() } {
            let data = batch_of(bytes, created);
            if type_is(data, created, b"text")
                && let Some(base) = font_size(data, created)
                && (scaled.len() < SCALED_CAPACITY || scaled.contains_key(&node))
            {
                scaled.insert(
                    node,
                    ScaledText {
                        owner,
                        base,
                        scaled: false,
                        scale: 1.0,
                        selectable: false,
                    },
                );
            }
        }
        if depth >= SCALE_DEPTH {
            continue;
        }
        let Some(list) = (unsafe { wire_child_list(bytes, spans.as_ptr(), spans.len(), node).as_ref() }) else {
            continue;
        };
        let data = batch_of(bytes, list);
        for position in 0..list.child_count {
            if queue.len() >= SCALE_NODES {
                break;
            }
            queue.push((child(data, list, position), depth + 1));
        }
    }
}

fn reapply_after_update(state: &mut State, id: i32, bytes: *const u8, length: usize, out: &mut Out, changed: &mut c_int) {
    let Some(patched) = state.patched.get_mut(&id) else { return };
    let Some(patch) = patched.written.take() else { return };
    if patched.applied == 0 {
        patched.written = Some(patch);
        return;
    }
    if *changed == 0 {
        out.put(unsafe { core::slice::from_raw_parts(bytes, length) });
    }
    *changed = 1;
    write_patch(out, id, patched, &patch);
}

#[unsafe(export_name = "wirePatchNodes")]
pub unsafe extern "C" fn wire_patch_nodes(
    bytes: *const u8,
    pointer: *const Span,
    count: usize,
    out: *mut Out,
    changed: *mut c_int,
    length: usize,
) {
    if PATCHER.get::<NodePatcher>().is_none() {
        return;
    }
    let (Some(out), Some(changed)) = (unsafe { out.as_mut() }, unsafe { changed.as_mut() }) else {
        return;
    };
    let spans = spans_of(pointer, count);
    let properties = properties::current();
    with_state(|state| {
        if state.prefixes.is_empty() {
            return;
        }
        for span in spans {
            let data = batch_of(bytes, span);
            if span.op == Op::Remove {
                for position in 0..span.child_count {
                    let removed = child(data, span, position);
                    state.patched.remove(&removed);
                    state.scaled.remove(&removed);
                }
                continue;
            }
            if span.op == Op::Update {
                reapply_after_update(state, span.id, bytes, length, out, changed);
                continue;
            }
            if span.op != Op::Create {
                continue;
            }
            let Some((identifier, kind)) = read_identifier(data, span) else {
                continue;
            };
            let Some(prefix) = key_offset(&state.prefixes, identifier) else {
                continue;
            };
            if state.patched.len() >= PATCHED_CAPACITY && !state.patched.contains_key(&span.id) {
                continue;
            }
            let mut patched = Patched {
                identifier: identifier.to_vec(),
                kind: kind.to_vec(),
                ..Patched::default()
            };
            capture_originals(data, span, &mut patched, &properties);
            if unsafe { super::format::wire_color_property(bytes, span, properties.background, &mut patched.background) } != 0
                && patched.background != 0
            {
                collect_see_through(bytes, spans, span.id, patched.background, 1, &mut patched.see_through);
            }
            collect_texts(&mut state.scaled, bytes, spans, span.id);
            let mut patch = AstralNodePatch::default();
            let result = call_patcher(prefix, span.id, identifier, kind, &mut patch);
            if result < 0 {
                wire_mark_refresh_busy();
            }
            if result > 0 {
                if *changed == 0 {
                    out.put(unsafe { core::slice::from_raw_parts(bytes, length) });
                }
                *changed = 1;
                write_patch(out, span.id, &mut patched, &patch);
                write_scaled(out, &mut state.scaled, span.id, &patch);
            }
            state.patched.insert(span.id, patched);
        }
    });
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_wire_refresh_node_patches(buffer: *mut AstralWireBuffer) -> c_int {
    let Some(buffer) = (unsafe { buffer.as_mut() }) else { return 0 };
    *buffer = AstralWireBuffer {
        bytes: core::ptr::null_mut(),
        length: 0,
    };
    if PATCHER.get::<NodePatcher>().is_none() {
        return 0;
    }
    let mut out = Out::batch();
    let any = with_state(|state| {
        let mut any = false;
        let prefixes = state.prefixes.clone();
        for (id, patched) in state.patched.iter_mut() {
            let mut patch = AstralNodePatch::default();
            let prefix = key_offset(&prefixes, &patched.identifier).unwrap_or(0);
            if call_patcher(prefix, *id, &patched.identifier, &patched.kind, &mut patch) < 0 {
                wire_mark_refresh_busy();
                continue;
            }
            let wrote = write_patch(&mut out, *id, patched, &patch);
            any |= write_scaled(&mut out, &mut state.scaled, *id, &patch) || wrote;
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
