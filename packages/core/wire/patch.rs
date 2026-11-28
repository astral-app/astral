use super::tag::{Op, Tag};
use core::ffi::{c_char, c_int};
use std::collections::HashMap;
use std::ffi::CString;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use super::abi::{
    AstralNodePatch, AstralPatchTarget, Callback, PATCH_BACKGROUND, PATCH_BORDER, PATCH_GRADIENT, PATCH_HIDDEN, PATCH_LABEL_COLOR,
    PATCH_LABEL_TEXT, PATCH_LABEL_TINT, PATCH_MARGIN, PATCH_OPACITY, PATCH_PADDING, PATCH_RADIUS, PATCH_SEE_THROUGH, PATCH_SELECTABLE,
    PATCH_TEXT_COLOR, PATCH_TEXT_SCALE, current_channel, wire_message_author, wire_row_message,
};
use super::cursor::Cursor;
use super::format::{Span, batch_of, child, spans_of, type_is, wire_child_list, wire_creation};
use super::ids::parse_id;
use super::out::{AstralWireBuffer, Out};
use super::properties::{self, AstralWireProperties};

pub type NodePatcher = unsafe extern "C" fn(*const AstralPatchTarget, *mut AstralNodePatch) -> c_int;

const MAX_PREFIXES: usize = 128;
const MAX_PREFIX_LENGTH: usize = 96;
const PATCHED_CAPACITY: usize = 512;
const SEEN_CAPACITY: usize = 4096;
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
    Flex,
    MinWidth,
}

impl Slot {
    const ALL: [Slot; 14] = [
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
        Slot::Flex,
        Slot::MinWidth,
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
            Slot::Flex => properties.flex,
            Slot::MinWidth => properties.min_width,
            Slot::Gradient => properties.background_gradient,
        }
    }
}
const SCALED_CAPACITY: usize = 2048;
const LABEL_DEPTH: u32 = 3;
const LABEL_NODES: usize = 32;
const LABELED_CAPACITY: usize = 1024;
const LABEL_FIELDS: u32 = PATCH_LABEL_TEXT | PATCH_LABEL_COLOR | PATCH_LABEL_TINT;
const SCALE_DEPTH: u32 = 8;
const SCALE_NODES: usize = 256;
const OWNER_DEPTH: usize = 32;
pub const IDENTIFIER_LENGTH: usize = 128;
pub const TYPE_LENGTH: usize = 16;

#[derive(Clone, Default)]
struct RawProp {
    tag: u8,
    value: Vec<u8>,
}

#[derive(Clone, Default)]
struct Patched {
    checked: Option<bool>,
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
    base: Option<f64>,
    scaled: bool,
    scale: f64,
    selectable: bool,
}

#[derive(Default)]
struct Labeled {
    owner: i32,
    image: bool,
    text: Option<RawProp>,
    color: Option<RawProp>,
    applied: bool,
}

#[derive(Default)]
struct State {
    prefixes: Vec<Vec<u8>>,
    patched: HashMap<i32, Patched>,
    seen: HashMap<i32, Patched>,
    scaled: HashMap<i32, ScaledText>,
    labeled: HashMap<i32, Labeled>,
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
    let promoted = with_state(|state| {
        state.prefixes = prefixes;
        promote_seen(state)
    })
    .unwrap_or(false);
    if promoted {
        wire_mark_refresh_busy();
    }
}

fn promote_seen(state: &mut State) -> bool {
    let matching: Vec<i32> = state
        .seen
        .iter()
        .filter(|(_, seen)| key_offset(&state.prefixes, &seen.identifier).is_some())
        .map(|(id, _)| *id)
        .collect();
    let mut promoted = false;
    for id in matching {
        if state.patched.len() >= PATCHED_CAPACITY {
            break;
        }
        if let Some(seen) = state.seen.remove(&id) {
            state.patched.insert(id, seen);
            promoted = true;
        }
    }
    promoted
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

pub const LABEL_PREFIX: &[u8] = b"label:";
pub const LABEL_END: u8 = 0x1f;

fn patch_identifier<'a>(bytes: &'a [u8], span: &Span) -> Option<(Vec<u8>, &'a [u8])> {
    let properties = properties::current();
    let mut cursor = Cursor::new(bytes, span.start + 5);
    let type_length = cursor.read16() as usize;
    if type_length >= TYPE_LENGTH {
        return None;
    }
    let kind = cursor.take(type_length)?;
    let count = cursor.read16();
    let mut label = None;
    for _ in 0..count {
        if cursor.failed() {
            break;
        }
        let property = cursor.read16();
        let tag = cursor.read8();
        if tag == Tag::String && (property == properties.accessibility_id || property == properties.accessibility_label) {
            let length = cursor.read32() as usize;
            let text = cursor.take(length)?;
            if property == properties.accessibility_id {
                return (length < IDENTIFIER_LENGTH).then(|| (text.to_vec(), kind));
            }
            label = Some(text);
            continue;
        }
        cursor.skip_value(tag, 0);
    }
    let label = label.filter(|label| label.len() + LABEL_PREFIX.len() + 1 < IDENTIFIER_LENGTH)?;
    Some(([LABEL_PREFIX, label, &[LABEL_END]].concat(), kind))
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
    checked_patch_target(prefix_length, identifier, kind, node, None)
}

fn checked_patch_target(prefix_length: usize, identifier: &CString, kind: &CString, node: i32, checked: Option<bool>) -> AstralPatchTarget {
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
        checked: checked.map_or(-1, c_int::from),
        label: core::ptr::null(),
    }
}

fn call_patcher(prefix_length: usize, node: i32, patched: &Patched, patch: &mut AstralNodePatch) -> c_int {
    let Some(patcher) = PATCHER.get::<NodePatcher>() else { return 0 };
    let (Ok(identifier), Ok(kind)) = (CString::new(patched.identifier.as_slice()), CString::new(patched.kind.as_slice())) else {
        return 0;
    };
    let target = checked_patch_target(prefix_length, &identifier, &kind, node, patched.checked);
    unsafe { patcher(&target, patch) }
}

fn checked_in(bytes: &[u8], span: &Span) -> Option<bool> {
    let wanted = properties::current().accessibility_checked;
    let (mut cursor, count) = super::format::property_cursor(bytes, span);
    for _ in 0..count {
        if cursor.failed() {
            break;
        }
        let property = cursor.read16();
        let tag = cursor.read8();
        if property == wanted && (tag == Tag::True || tag == Tag::False) {
            return Some(tag == Tag::True);
        }
        cursor.skip_value(tag, 0);
    }
    None
}

fn raw_props(bytes: &[u8], span: &Span, wanted: &[u16]) -> Vec<(u16, RawProp)> {
    let (mut cursor, count) = super::format::property_cursor(bytes, span);
    let mut found = Vec::new();
    for _ in 0..count {
        if cursor.failed() {
            break;
        }
        let property = cursor.read16();
        let tag = cursor.read8();
        let start = cursor.offset();
        cursor.skip_value(tag, 0);
        if wanted.contains(&property)
            && let Some(value) = bytes.get(start..cursor.offset()).filter(|value| value.len() <= 512)
        {
            found.push((
                property,
                RawProp {
                    tag,
                    value: value.to_vec(),
                },
            ));
        }
    }
    found
}

fn note_label_originals(labeled: &mut Labeled, bytes: &[u8], span: &Span) {
    let properties = properties::current();
    let (text, color) = if labeled.image {
        (0, properties.tint)
    } else {
        (properties.text, properties.color)
    };
    for (property, raw) in raw_props(bytes, span, &[text, color]) {
        if property == text && text != 0 {
            labeled.text = Some(raw);
        } else if property == color {
            labeled.color = Some(raw);
        }
    }
}

fn collect_labels(labeled: &mut HashMap<i32, Labeled>, bytes: *const u8, spans: &[Span], owner: i32) {
    let mut queue = vec![(owner, 0u32)];
    let mut cursor = 0;
    while cursor < queue.len() {
        let (node, depth) = queue[cursor];
        cursor += 1;
        if let Some(created) = unsafe { wire_creation(spans.as_ptr(), spans.len(), node).as_ref() } {
            let data = batch_of(bytes, created);
            let image = type_is(data, created, b"image");
            if node != owner && (image || type_is(data, created, b"text")) && labeled.len() < LABELED_CAPACITY {
                let mut entry = Labeled {
                    owner,
                    image,
                    ..Labeled::default()
                };
                note_label_originals(&mut entry, data, created);
                labeled.insert(node, entry);
            }
        }
        if depth >= LABEL_DEPTH {
            continue;
        }
        let Some(list) = (unsafe { wire_child_list(bytes, spans.as_ptr(), spans.len(), node).as_ref() }) else {
            continue;
        };
        let data = batch_of(bytes, list);
        for position in 0..list.child_count {
            if queue.len() >= LABEL_NODES {
                break;
            }
            queue.push((child(data, list, position), depth + 1));
        }
    }
}

fn put_raw(out: &mut Out, property: u16, raw: Option<&RawProp>) {
    out.put16(property);
    match raw {
        Some(raw) => {
            out.put8(raw.tag);
            out.put(&raw.value);
        }
        None => out.put_tag(Tag::Null),
    }
}

fn label_text(patch: &AstralNodePatch) -> &[u8] {
    let length = patch
        .label_text
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(patch.label_text.len());
    unsafe { core::slice::from_raw_parts(patch.label_text.as_ptr().cast::<u8>(), length) }
}

fn write_labels(out: &mut Out, labeled: &mut HashMap<i32, Labeled>, owner: i32, patch: &AstralNodePatch, force: bool) -> bool {
    let properties = properties::current();
    let wanted = patch.fields & LABEL_FIELDS;
    let mut wrote = false;
    for (id, entry) in labeled.iter_mut().filter(|(_, entry)| entry.owner == owner) {
        if wanted == 0 && !entry.applied {
            continue;
        }
        if wanted != 0 && entry.applied && !force {
            continue;
        }
        if entry.image {
            out.update(*id, 1);
            if patch.fields & PATCH_LABEL_TINT != 0 {
                put_long(out, properties.tint, patch.label_tint);
            } else {
                put_raw(out, properties.tint, entry.color.as_ref());
            }
        } else {
            out.update(*id, 2);
            if patch.fields & PATCH_LABEL_TEXT != 0 {
                out.prop_string(properties.text, label_text(patch));
            } else {
                put_raw(out, properties.text, entry.text.as_ref());
            }
            if patch.fields & PATCH_LABEL_COLOR != 0 {
                put_long(out, properties.color, patch.label_color);
            } else {
                put_raw(out, properties.color, entry.color.as_ref());
            }
        }
        entry.applied = wanted != 0;
        wrote = true;
    }
    wrote
}

fn remember_seen(state: &mut State, seen_in: (*const u8, &[Span]), span: &Span, identifier: &[u8], kind: &[u8]) {
    if state.seen.len() >= SEEN_CAPACITY && !state.seen.contains_key(&span.id) {
        return;
    }
    let (bytes, spans) = seen_in;
    let data = batch_of(bytes, span);
    let mut seen = Patched {
        identifier: identifier.to_vec(),
        kind: kind.to_vec(),
        checked: checked_in(data, span),
        ..Patched::default()
    };
    capture_originals(data, span, &mut seen, &properties::current());
    if seen.checked.is_some() {
        collect_labels(&mut state.labeled, bytes, spans, span.id);
    }
    state.seen.insert(span.id, seen);
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
        (
            PATCH_HIDDEN,
            Slot::Width.bit() | Slot::Height.bit() | Slot::Clip.bit() | Slot::Flex.bit() | Slot::MinWidth.bit(),
        ),
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
        Slot::Width | Slot::Height | Slot::Flex | Slot::MinWidth => out.prop_number(property, 0.0),
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
        let rescale = text.base.is_some() && !((scale == 1.0 && !text.scaled) || (text.scaled && text.scale == scale));
        let reselect = text.selectable != selectable;
        if !rescale && !reselect {
            continue;
        }
        out.update(*id, u16::from(rescale) + u16::from(reselect));
        if rescale {
            out.prop_number(properties.font_size, text.base.unwrap_or_default() * scale);
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

fn remember_text(scaled: &mut HashMap<i32, ScaledText>, node: i32, owner: i32, base: Option<f64>) {
    if scaled.len() >= SCALED_CAPACITY && !scaled.contains_key(&node) {
        return;
    }
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

fn styles_texts(patched: &Patched) -> bool {
    patched
        .written
        .as_ref()
        .is_some_and(|patch| patch.fields & (PATCH_SELECTABLE | PATCH_TEXT_SCALE) != 0)
}

fn text_owner(state: &State, bytes: *const u8, spans: &[Span], node: i32) -> Option<i32> {
    let mut current = node;
    for _ in 0..OWNER_DEPTH {
        let parent = unsafe { super::format::wire_parent_of(bytes, spans.as_ptr(), spans.len(), current) };
        current = if parent > 0 {
            parent
        } else {
            super::children::wire_discord_parent(current)
        };
        if current <= 0 {
            return None;
        }
        if state.patched.get(&current).is_some_and(styles_texts) {
            return Some(current);
        }
    }
    None
}

fn adopt_late_texts(state: &mut State, bytes: *const u8, spans: &[Span], out: &mut Out, changed: &mut c_int, length: usize) {
    if !state.patched.values().any(styles_texts) {
        return;
    }
    let mut owners = Vec::new();
    for span in spans.iter().filter(|span| span.op == Op::Create) {
        let data = batch_of(bytes, span);
        if state.scaled.contains_key(&span.id) || !type_is(data, span, b"text") {
            continue;
        }
        let Some(owner) = text_owner(state, bytes, spans, span.id) else {
            continue;
        };
        remember_text(&mut state.scaled, span.id, owner, font_size(data, span));
        if !owners.contains(&owner) {
            owners.push(owner);
        }
    }
    for owner in owners {
        let Some(patch) = state.patched.get(&owner).and_then(|patched| patched.written) else {
            continue;
        };
        let mut staged = Out::default();
        if write_scaled(&mut staged, &mut state.scaled, owner, &patch) {
            if *changed == 0 {
                out.put(unsafe { core::slice::from_raw_parts(bytes, length) });
            }
            *changed = 1;
            out.put(unsafe { core::slice::from_raw_parts(staged.bytes, staged.length) });
        }
        staged.discard();
    }
}

fn collect_texts(scaled: &mut HashMap<i32, ScaledText>, bytes: *const u8, spans: &[Span], owner: i32) {
    let mut queue = vec![(owner, 0u32)];
    let mut cursor = 0;
    while cursor < queue.len() {
        let (node, depth) = queue[cursor];
        cursor += 1;
        if let Some(created) = unsafe { wire_creation(spans.as_ptr(), spans.len(), node).as_ref() } {
            let data = batch_of(bytes, created);
            if type_is(data, created, b"text") {
                remember_text(scaled, node, owner, font_size(data, created));
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

fn label_differs(first: &AstralNodePatch, second: &AstralNodePatch) -> bool {
    (first.fields & LABEL_FIELDS) != (second.fields & LABEL_FIELDS)
        || first.label_text != second.label_text
        || first.label_color != second.label_color
        || first.label_tint != second.label_tint
}

fn begin_output(out: &mut Out, changed: &mut c_int, bytes: *const u8, length: usize) {
    if *changed == 0 {
        out.put(unsafe { core::slice::from_raw_parts(bytes, length) });
    }
    *changed = 1;
}

fn repatch_checked(state: &mut State, id: i32, checked: bool, bytes: *const u8, length: usize, out: &mut Out, changed: &mut c_int) {
    let prefixes = state.prefixes.clone();
    let Some(patched) = state.patched.get_mut(&id) else { return };
    patched.checked = Some(checked);
    let prefix = key_offset(&prefixes, &patched.identifier).unwrap_or(0);
    let mut patch = AstralNodePatch::default();
    if call_patcher(prefix, id, patched, &mut patch) < 0 {
        wire_mark_refresh_busy();
        return;
    }
    begin_output(out, changed, bytes, length);
    patched.written = None;
    write_patch(out, id, patched, &patch);
    write_scaled(out, &mut state.scaled, id, &patch);
    write_labels(out, &mut state.labeled, id, &patch, true);
}

fn relabel_after_child_update(
    state: &mut State,
    span: &Span,
    data: &[u8],
    bytes: *const u8,
    length: usize,
    out: &mut Out,
    changed: &mut c_int,
) {
    let Some(entry) = state.labeled.get_mut(&span.id) else { return };
    note_label_originals(entry, data, span);
    let owner = entry.owner;
    let applied = entry.applied;
    let Some(patch) = state.patched.get(&owner).and_then(|patched| patched.written) else {
        return;
    };
    if !applied || patch.fields & LABEL_FIELDS == 0 {
        return;
    }
    begin_output(out, changed, bytes, length);
    write_labels(out, &mut state.labeled, owner, &patch, true);
}

fn reapply_after_update(state: &mut State, id: i32, bytes: *const u8, length: usize, out: &mut Out, changed: &mut c_int) {
    let Some(patched) = state.patched.get_mut(&id) else { return };
    let Some(patch) = patched.written.take() else { return };
    if patched.applied == 0 {
        patched.written = Some(patch);
        return;
    }
    begin_output(out, changed, bytes, length);
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
        for span in spans {
            let data = batch_of(bytes, span);
            if span.op == Op::Remove {
                for position in 0..span.child_count {
                    let removed = child(data, span, position);
                    state.patched.remove(&removed);
                    state.seen.remove(&removed);
                    state.scaled.remove(&removed);
                    state.labeled.remove(&removed);
                }
                continue;
            }
            if span.op == Op::Update {
                state.seen.remove(&span.id);
                let checked = checked_in(data, span);
                let flipped =
                    checked.is_some_and(|checked| state.patched.get(&span.id).is_some_and(|patched| patched.checked != Some(checked)));
                match checked {
                    Some(checked) if flipped => repatch_checked(state, span.id, checked, bytes, length, out, changed),
                    _ => reapply_after_update(state, span.id, bytes, length, out, changed),
                }
                relabel_after_child_update(state, span, data, bytes, length, out, changed);
                continue;
            }
            if span.op != Op::Create {
                continue;
            }
            let Some((identifier, kind)) = patch_identifier(data, span) else {
                continue;
            };
            let identifier = identifier.as_slice();
            let Some(prefix) = key_offset(&state.prefixes, identifier) else {
                remember_seen(state, (bytes, spans), span, identifier, kind);
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
            patched.checked = checked_in(data, span);
            if unsafe { super::format::wire_color_property(bytes, span, properties.background, &mut patched.background) } != 0
                && patched.background != 0
            {
                collect_see_through(bytes, spans, span.id, patched.background, 1, &mut patched.see_through);
            }
            collect_texts(&mut state.scaled, bytes, spans, span.id);
            collect_labels(&mut state.labeled, bytes, spans, span.id);
            let mut patch = AstralNodePatch::default();
            let result = call_patcher(prefix, span.id, &patched, &mut patch);
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
                write_labels(out, &mut state.labeled, span.id, &patch, true);
            }
            state.patched.insert(span.id, patched);
        }
        adopt_late_texts(state, bytes, spans, out, changed, length);
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
            if call_patcher(prefix, *id, patched, &mut patch) < 0 {
                wire_mark_refresh_busy();
                continue;
            }
            let labels_changed = patched.written.is_none_or(|written| label_differs(&written, &patch));
            let wrote = write_patch(&mut out, *id, patched, &patch);
            let relabeled = write_labels(&mut out, &mut state.labeled, *id, &patch, labels_changed);
            any |= write_scaled(&mut out, &mut state.scaled, *id, &patch) || wrote || relabeled;
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
