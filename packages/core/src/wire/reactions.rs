use core::ffi::{c_char, c_int};
use std::collections::{HashMap, HashSet};
use std::ffi::CString;
use std::sync::Mutex;
use std::sync::atomic::{AtomicI32, Ordering};

use super::abi::{Callback, current_channel};
use super::emoji::glyph;
use super::format::{Span, batch_of, child, find_string, spans_of, type_is, wireChildList, wireCreation};
use super::out::{AstralWireBuffer, Out};
use super::properties;
use super::tween::{wireTweenForget, wireTweenIn};

pub const FACES: usize = 5;

#[repr(C)]
pub struct AstralReactionContext {
    pub message_id: u64,
    pub channel_id: u64,
    pub emoji: *const c_char,
    pub burst: c_int,
    pub count: u32,
}

#[repr(C)]
pub struct AstralReactionDecoration {
    pub images: [[c_char; 512]; FACES],
    pub image_count: u32,
    pub total: u32,
    pub stale: c_int,
}

pub type ReactionDecorator = unsafe extern "C" fn(*const AstralReactionContext, *mut AstralReactionDecoration) -> c_int;

const MAX_PILLS: usize = 256;
const MAX_PILL_CHILDREN: usize = 8;
const MAX_RETRIES: u8 = 20;
const FIRST_STACK_ID: i32 = 0x7c000000;
const STACK_ID_RANGE: i32 = 0x00f00000;
const REACTION_PREFIX: &[u8] = b"message.reaction:";
const FACE_SIZE: f64 = 16.2;
const FACE_STEP: f64 = 4.5;
const BADGE_FONT_SIZE: f64 = 7.2;
const TRAILING_INSET: f64 = -2.0;
const FNV_OFFSET: u64 = 1469598103934665603;
const FNV_PRIME: u64 = 1099511628211;

#[derive(Clone, Default)]
struct Pill {
    message: u64,
    emoji: Vec<u8>,
    burst: bool,
    children: Vec<i32>,
    count_text: i32,
    total: u32,
    nodes: Vec<i32>,
    shown: u64,
    dirty: bool,
    pending: bool,
    tries: u8,
}

static PILLS: Mutex<Option<HashMap<i32, Pill>>> = Mutex::new(None);
static DECORATOR: Callback = Callback::new();
static NEXT_STACK_ID: AtomicI32 = AtomicI32::new(FIRST_STACK_ID);

fn with_pills<T>(work: impl FnOnce(&mut HashMap<i32, Pill>) -> T) -> Option<T> {
    let mut pills = PILLS.lock().ok()?;
    Some(work(pills.get_or_insert_with(HashMap::new)))
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_wire_set_reaction_decorator(decorator: Option<ReactionDecorator>) {
    DECORATOR.set(decorator);
}

fn stack_id() -> i32 {
    let id = NEXT_STACK_ID.fetch_add(1, Ordering::Relaxed);
    if id + 1 >= FIRST_STACK_ID + STACK_ID_RANGE {
        NEXT_STACK_ID.store(FIRST_STACK_ID, Ordering::Relaxed);
    }
    id
}

fn parse_count(text: &[u8]) -> u32 {
    text.iter()
        .take_while(|byte| byte.is_ascii_digit())
        .fold(0u32, |value, digit| value.wrapping_mul(10).wrapping_add(u32::from(digit - b'0')))
}

fn parse_reaction(bytes: &[u8], span: &Span) -> Option<Pill> {
    let identifier = find_string(bytes, span, properties::current().accessibilityId)?;
    if identifier.len() >= 256 {
        return None;
    }
    let rest = identifier.strip_prefix(REACTION_PREFIX).filter(|rest| !rest.is_empty())?;
    let mut fields = rest.splitn(4, |byte| *byte == b':');
    let message = fields.next().unwrap_or_default();
    let kind = fields.next().unwrap_or_default();
    let category = fields.next().unwrap_or_default();
    let value = fields.next().unwrap_or_default();
    let message: u64 = core::str::from_utf8(message).ok()?.parse().ok()?;
    if message == 0 || value.is_empty() {
        return None;
    }
    let emoji = if category == b"unicode" {
        glyph(&value[..value.len().min(127)], 96)
    } else {
        let id = value.rsplit(|byte| *byte == b':').next().unwrap_or(value);
        [b"e:".as_slice(), &id[..id.len().min(93)]].concat()
    };
    (!emoji.is_empty()).then(|| Pill {
        message,
        emoji,
        burst: kind == b"burst",
        ..Pill::default()
    })
}

fn first_text(bytes: *const u8, spans: &[Span], id: i32, depth: u32) -> i32 {
    if let Some(creation) = unsafe { wireCreation(spans.as_ptr(), spans.len(), id).as_ref() }
        && type_is(batch_of(bytes, creation), creation, b"text")
    {
        return id;
    }
    if depth >= 3 {
        return 0;
    }
    let Some(list) = (unsafe { wireChildList(bytes, spans.as_ptr(), spans.len(), id).as_ref() }) else {
        return 0;
    };
    let data = batch_of(bytes, list);
    (0..list.child_count)
        .map(|position| first_text(bytes, spans, child(data, list, position), depth + 1))
        .find(|found| *found != 0)
        .unwrap_or(0)
}

fn adopt_children(pill: &mut Pill, bytes: &[u8], list: &Span) {
    pill.children = (0..list.child_count)
        .map(|position| child(bytes, list, position))
        .filter(|id| !pill.nodes.contains(id))
        .take(MAX_PILL_CHILDREN)
        .collect();
}

fn locate_count(pill: &mut Pill, bytes: *const u8, spans: &[Span]) {
    let Some(last) = pill.children.last().copied().filter(|_| pill.children.len() >= 2) else {
        return;
    };
    let text = first_text(bytes, spans, last, 0);
    let Some(creation) = (text != 0)
        .then(|| unsafe { wireCreation(spans.as_ptr(), spans.len(), text).as_ref() })
        .flatten()
    else {
        return;
    };
    pill.count_text = text;
    if let Some(value) = find_string(batch_of(bytes, creation), creation, properties::current().text) {
        pill.total = parse_count(value);
    }
}

fn register(pills: &mut HashMap<i32, Pill>, bytes: *const u8, spans: &[Span], span: &Span) {
    let Some(mut pill) = parse_reaction(batch_of(bytes, span), span) else {
        return;
    };
    if pills.len() >= MAX_PILLS && !pills.contains_key(&span.id) {
        return;
    }
    if let Some(list) = unsafe { wireChildList(bytes, spans.as_ptr(), spans.len(), span.id).as_ref() } {
        adopt_children(&mut pill, batch_of(bytes, list), list);
    }
    locate_count(&mut pill, bytes, spans);
    pill.dirty = !pill.children.is_empty() && pill.total > 0;
    pills.insert(span.id, pill);
}

fn image_of(images: &[c_char; 512]) -> &[u8] {
    let length = images.iter().position(|byte| *byte == 0).unwrap_or(511);
    unsafe { core::slice::from_raw_parts(images.as_ptr().cast::<u8>(), length) }
}

fn decoration_hash(decoration: &AstralReactionDecoration) -> u64 {
    let faces = (decoration.image_count as usize).min(FACES);
    let hash = decoration.images[..faces]
        .iter()
        .fold(FNV_OFFSET ^ u64::from(decoration.total), |hash, image| {
            let hash = image_of(image)
                .iter()
                .fold(hash, |hash, byte| (hash ^ u64::from(*byte)).wrapping_mul(FNV_PRIME));
            (hash ^ 0xff).wrapping_mul(FNV_PRIME)
        });
    if hash == 0 { 1 } else { hash }
}

fn remove_stack(out: &mut Out, pill: &mut Pill) {
    if pill.nodes.is_empty() {
        return;
    }
    for node in &pill.nodes {
        wireTweenForget(*node);
    }
    out.put8(4);
    out.put32(pill.nodes.len() as u32);
    for node in &pill.nodes {
        out.put(&node.to_le_bytes());
    }
    pill.nodes.clear();
}

fn new_node(out: &mut Out, pill: &mut Pill, kind: &[u8], properties: u16) -> i32 {
    let id = stack_id();
    out.put8(1);
    out.put(&id.to_le_bytes());
    out.put16(kind.len() as u16);
    out.put(kind);
    out.prop_count(properties);
    if pill.nodes.len() < FACES * 2 + 4 {
        pill.nodes.push(id);
    }
    id
}

fn put_long(out: &mut Out, property: u16, value: u32) {
    out.put16(property);
    out.put8(4);
    out.put(&i64::from(value).to_le_bytes());
}

fn put_children(out: &mut Out, parent: i32, children: &[i32]) {
    out.put8(3);
    out.put(&parent.to_le_bytes());
    out.put32(children.len() as u32);
    for id in children {
        out.put(&id.to_le_bytes());
    }
}

fn put_face(out: &mut Out, pill: &mut Pill, image: &[u8], first: bool) -> i32 {
    let properties = properties::current();
    let face = new_node(out, pill, b"image", 8);
    out.prop_string(properties.uri, image);
    out.prop_string(properties.pool, b"small");
    out.prop_number(properties.width, FACE_SIZE);
    out.prop_number(properties.height, FACE_SIZE);
    out.prop_number(properties.radius, FACE_SIZE / 2.0);
    out.prop_string(properties.scale, b"fill");
    out.prop_quad(
        properties.margin,
        [if first { 0.0 } else { (FACE_STEP - FACE_SIZE) as f32 }, 0.0, 0.0, 0.0],
    );
    out.prop_number(properties.shrink, 0.0);
    face
}

fn put_badge(out: &mut Out, pill: &mut Pill, overflow: u32) -> i32 {
    let properties = properties::current();
    let text = new_node(out, pill, b"text", 6);
    put_long(out, properties.color, 0xFFFFFFFF);
    out.prop_number(properties.fontSize, BADGE_FONT_SIZE);
    out.prop_int(properties.fontWeight, 600);
    out.prop_int(properties.maxLines, 1);
    out.prop_string(properties.text, format!("+{overflow}").as_bytes());
    out.put16(properties.notoFallback);
    out.put8(2);
    let badge = new_node(out, pill, b"view", 8);
    put_long(out, properties.background, 0xFF000000);
    out.prop_number(properties.width, FACE_SIZE);
    out.prop_number(properties.height, FACE_SIZE);
    out.prop_number(properties.radius, FACE_SIZE / 2.0);
    out.prop_string(properties.align, b"center");
    out.prop_string(properties.justify, b"center");
    out.prop_quad(properties.margin, [(FACE_STEP - FACE_SIZE) as f32, 0.0, 0.0, 0.0]);
    out.prop_number(properties.shrink, 0.0);
    put_children(out, badge, &[text]);
    badge
}

fn put_stack(out: &mut Out, id: i32, pill: &mut Pill, decoration: &AstralReactionDecoration) {
    let properties = properties::current();
    let faces = (decoration.image_count as usize).min(FACES);
    let mut members: Vec<i32> = (0..faces)
        .map(|face| put_face(out, pill, image_of(&decoration.images[face]), face == 0))
        .collect();
    if decoration.total as usize > faces && faces == FACES {
        members.push(put_badge(out, pill, decoration.total - faces as u32));
    }
    let stack = new_node(out, pill, b"view", 7);
    out.prop_string(properties.direction, b"row");
    out.prop_string(properties.align, b"center");
    out.prop_quad(properties.margin, [0.0; 4]);
    out.prop_number(properties.shrink, 0.0);
    out.put16(properties.clip);
    out.put8(2);
    out.prop_number(properties.opacity, 0.0);
    out.prop_number(properties.maxWidth, 0.0);
    let width = if members.is_empty() {
        0.0
    } else {
        FACE_SIZE + (members.len() - 1) as f64 * FACE_STEP
    };
    wireTweenIn(stack, width, TRAILING_INSET);
    put_children(out, stack, &members);
    let mut children = pill.children.clone();
    children.push(stack);
    put_children(out, id, &children);
}

fn restore(out: &mut Out, id: i32, pill: &mut Pill) {
    remove_stack(out, pill);
    put_children(out, id, &pill.children);
    pill.shown = 0;
}

fn refresh_pill(out: &mut Out, id: i32, pill: &mut Pill, force: bool) -> bool {
    let Some(decorator) = DECORATOR.get::<ReactionDecorator>() else {
        return false;
    };
    if pill.total == 0 {
        if pill.nodes.is_empty() {
            return false;
        }
        restore(out, id, pill);
        return true;
    }
    let Ok(emoji) = CString::new(pill.emoji.clone()) else {
        return false;
    };
    let context = AstralReactionContext {
        message_id: pill.message,
        channel_id: current_channel(),
        emoji: emoji.as_ptr(),
        burst: c_int::from(pill.burst),
        count: pill.total,
    };
    let mut decoration: AstralReactionDecoration = unsafe { core::mem::zeroed() };
    if unsafe { decorator(&context, &mut decoration) } == 0 || decoration.image_count == 0 {
        pill.pending = true;
        if !force || pill.nodes.is_empty() {
            return false;
        }
        remove_stack(out, pill);
        pill.shown = 0;
        return true;
    }
    pill.pending = decoration.stale != 0;
    if !pill.pending {
        pill.tries = 0;
    }
    if decoration.total == 0 {
        decoration.total = pill.total;
    }
    let hash = decoration_hash(&decoration);
    if !force && hash == pill.shown && !pill.nodes.is_empty() {
        return false;
    }
    remove_stack(out, pill);
    put_stack(out, id, pill, &decoration);
    pill.shown = hash;
    true
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wireReactionAvatars(
    bytes: *const u8,
    pointer: *const Span,
    count: usize,
    out: *mut Out,
    changed: *mut c_int,
    length: usize,
) {
    if DECORATOR.get::<ReactionDecorator>().is_none() {
        return;
    }
    let (Some(out), Some(changed)) = (unsafe { out.as_mut() }, unsafe { changed.as_mut() }) else {
        return;
    };
    let spans = spans_of(pointer, count);
    let text_key = properties::current().text;
    with_pills(|pills| {
        for span in spans {
            let data = batch_of(bytes, span);
            match span.op {
                4 => {
                    for position in 0..span.child_count {
                        if let Some(pill) = pills.remove(&child(data, span, position)) {
                            for node in pill.nodes {
                                wireTweenForget(node);
                            }
                        }
                    }
                }
                1 if type_is(data, span, b"view") => register(pills, bytes, spans, span),
                2 => {
                    let Some(pill) = pills.values_mut().find(|pill| pill.count_text == span.id) else {
                        continue;
                    };
                    let Some(value) = find_string(data, span, text_key) else { continue };
                    let total = parse_count(value);
                    if total != pill.total {
                        pill.total = total;
                        pill.dirty = true;
                    }
                }
                _ => {}
            }
        }
        let mut forced = HashSet::new();
        for span in spans.iter().filter(|span| span.op == 3) {
            if !unsafe { wireCreation(spans.as_ptr(), spans.len(), span.id) }.is_null() {
                continue;
            }
            let Some(pill) = pills.get_mut(&span.id) else { continue };
            adopt_children(pill, batch_of(bytes, span), span);
            locate_count(pill, bytes, spans);
            if !pill.nodes.is_empty() {
                forced.insert(span.id);
            }
            pill.dirty = true;
        }
        for (id, pill) in pills.iter_mut().filter(|(_, pill)| pill.dirty) {
            pill.dirty = false;
            let mut update = Out::default();
            if refresh_pill(&mut update, *id, pill, forced.contains(id)) && update.failed == 0 {
                if *changed == 0 {
                    out.put(unsafe { core::slice::from_raw_parts(bytes, length) });
                }
                *changed = 1;
                out.put(unsafe { core::slice::from_raw_parts(update.bytes, update.length) });
            }
            update.discard();
        }
    });
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_wire_refresh_reactions(buffer: *mut AstralWireBuffer) -> c_int {
    let Some(buffer) = (unsafe { buffer.as_mut() }) else { return 0 };
    *buffer = AstralWireBuffer {
        bytes: core::ptr::null_mut(),
        length: 0,
    };
    let decorating = DECORATOR.get::<ReactionDecorator>().is_some();
    let mut out = Out::batch();
    let sent = with_pills(|pills| {
        let mut sent = false;
        for (id, pill) in pills.iter_mut().filter(|(_, pill)| !pill.children.is_empty()) {
            pill.tries = 0;
            if decorating {
                sent |= refresh_pill(&mut out, *id, pill, false);
            }
            if !pill.nodes.is_empty() && (!decorating || pill.pending) {
                restore(&mut out, *id, pill);
                sent = true;
            }
        }
        sent
    })
    .unwrap_or(false);
    if !sent {
        out.discard();
        return 0;
    }
    out.finish(buffer) as c_int
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_wire_pending_reactions() -> c_int {
    if DECORATOR.get::<ReactionDecorator>().is_none() {
        return 0;
    }
    with_pills(|pills| pills.values().filter(|pill| pill.pending).count() as c_int).unwrap_or(0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_wire_retry_reactions(buffer: *mut AstralWireBuffer) -> c_int {
    let Some(buffer) = (unsafe { buffer.as_mut() }) else { return 0 };
    *buffer = AstralWireBuffer {
        bytes: core::ptr::null_mut(),
        length: 0,
    };
    if astral_wire_pending_reactions() == 0 {
        return 0;
    }
    let mut out = Out::batch();
    let sent = with_pills(|pills| {
        let mut sent = false;
        for (id, pill) in pills.iter_mut().filter(|(_, pill)| pill.pending) {
            pill.tries = pill.tries.saturating_add(1);
            if pill.tries > MAX_RETRIES {
                pill.pending = false;
                continue;
            }
            sent |= refresh_pill(&mut out, *id, pill, false);
        }
        sent
    })
    .unwrap_or(false);
    if !sent {
        out.discard();
        return 0;
    }
    out.finish(buffer) as c_int
}
