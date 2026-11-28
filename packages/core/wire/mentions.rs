use super::tag::{Op, Tag};
use core::ffi::{c_char, c_int};
use std::collections::HashMap;
use std::ffi::CString;
use std::sync::Mutex;

use super::abi::{AstralMentionContext, AstralMentionDecoration, Callback, current_channel};
use super::cursor::Cursor;
use super::format::{Span, batch_of, child, property_cursor, spans_of, type_is};
use super::out::{AstralWireBuffer, Out};
use super::properties::{self, AstralWireProperties};

const OPAQUE_ALPHA: u32 = 0xFF;

pub type MentionDecorator = unsafe extern "C" fn(*const AstralMentionContext, *mut AstralMentionDecoration) -> c_int;
pub type TextColorizer = unsafe extern "C" fn(u64, u64, *mut u32) -> c_int;

const MAX_SPANS: u32 = 64;
const MAX_TEXT: usize = 256;
const MAX_PENDING: usize = 64;
const MAX_RETRIES: u32 = 30;
const MAX_KEPT_FIELDS: usize = 24;
const MAX_KEPT_CHIP_FIELDS: usize = 16;
const CHIP_TINT: &str = "_chip_";

static DECORATOR: Callback = Callback::new();
static COLORIZER: Callback = Callback::new();
static PENDING: Mutex<Option<HashMap<i32, Pending>>> = Mutex::new(None);

struct Pending {
    message: u64,
    creation: Vec<u8>,
    tries: u32,
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_wire_set_mention_decorator(decorator: Option<MentionDecorator>) {
    DECORATOR.set(decorator);
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_wire_set_message_text_colorizer(colorizer: Option<TextColorizer>) {
    COLORIZER.set(colorizer);
}

fn active() -> bool {
    DECORATOR.get::<MentionDecorator>().is_some() || COLORIZER.get::<TextColorizer>().is_some()
}

#[derive(Default)]
struct Item {
    start: usize,
    end: usize,
    text: Option<(usize, usize)>,
    chip: bool,
    chip_part: bool,
    color: u32,
}

#[derive(Default, Clone, Copy)]
struct Change {
    drop_at: bool,
    color: u32,
    background: u32,
}

fn read_items(cursor: &mut Cursor, properties: &AstralWireProperties) -> Option<Vec<Item>> {
    let total = cursor.read32();
    if total > MAX_SPANS {
        return None;
    }
    let bytes = cursor.bytes();
    let mut items = Vec::new();
    for _ in 0..total {
        let start = cursor.offset();
        if cursor.read8() != Tag::Map {
            return None;
        }
        let mut item = Item { start, ..Item::default() };
        let (mut background, mut link) = (false, false);
        let entries = cursor.read32();
        for _ in 0..entries {
            if cursor.failed() {
                return None;
            }
            let key = cursor.read16();
            let tag = cursor.read8();
            if key == properties.text && tag == Tag::String {
                let length = cursor.read32() as usize;
                let at = cursor.offset();
                cursor.take(length)?;
                item.text = Some((at, length));
                continue;
            }
            background |= key == properties.bg;
            link |= key == properties.link;
            if key == properties.color
                && (tag == Tag::Int || tag == Tag::Long)
                && let Some(color) = bytes.get(cursor.offset()..cursor.offset() + 4)
            {
                item.color = u32::from_le_bytes(color.try_into().ok()?);
            }
            cursor.skip_value(tag, 1);
        }
        item.end = cursor.offset();
        item.chip_part = background && link;
        item.chip = item.chip_part && item.text.is_some_and(|(at, length)| length > 1 && bytes[at] == b'@');
        items.push(item);
    }
    (!cursor.failed()).then_some(items)
}

fn blended(base: u32, tint: u32) -> u32 {
    let amount = tint >> 24;
    if amount == OPAQUE_ALPHA {
        return tint;
    }
    (0..=16).step_by(8).fold(base & 0xFF00_0000, |result, shift| {
        let from = (base >> shift) & 0xFF;
        let to = (tint >> shift) & 0xFF;
        result | ((from * (255 - amount) + to * amount + 127) / 255) << shift
    })
}

fn kept_fields(bytes: &[u8], item: &Item, limit: usize, keep: impl Fn(u16) -> bool) -> Vec<(usize, usize)> {
    let mut cursor = Cursor::new(&bytes[..item.end], item.start + 1);
    let entries = cursor.read32();
    let mut kept = Vec::new();
    for _ in 0..entries {
        if cursor.failed() || kept.len() >= limit {
            break;
        }
        let at = cursor.offset();
        let key = cursor.read16();
        let tag = cursor.read8();
        cursor.skip_value(tag, 1);
        if keep(key) {
            kept.push((at, cursor.offset()));
        }
    }
    kept
}

fn put_long(out: &mut Out, key: u16, value: u32) {
    out.put16(key);
    out.put_tag(Tag::Long);
    out.put(&i64::from(value).to_le_bytes());
}

fn put_span(out: &mut Out, bytes: &[u8], item: &Item, change: Change, properties: &AstralWireProperties) {
    if change.color == 0 && change.background == 0 && !change.drop_at {
        out.put(&bytes[item.start..item.end]);
        return;
    }
    let kept = kept_fields(bytes, item, MAX_KEPT_FIELDS, |key| {
        !((change.color != 0 && key == properties.color)
            || (change.background != 0 && key == properties.bg)
            || (change.drop_at && key == properties.text))
    });
    let added = u32::from(change.color != 0) + u32::from(change.background != 0) + u32::from(change.drop_at);
    out.put_tag(Tag::Map);
    out.put32(kept.len() as u32 + added);
    for (start, end) in kept {
        out.put(&bytes[start..end]);
    }
    if change.drop_at
        && let Some((at, length)) = item.text
    {
        out.prop_string(properties.text, &bytes[at + 1..at + length]);
    }
    if change.color != 0 {
        put_long(out, properties.color, change.color);
    }
    if change.background != 0 {
        out.prop_int(properties.bg, change.background as i32);
    }
}

fn chip_uri(image: &[u8], color: u32) -> Vec<u8> {
    let Ok(text) = core::str::from_utf8(image) else {
        return image.to_vec();
    };
    match text.find(CHIP_TINT) {
        Some(at) if color != 0 => format!("{}_{:06X}_{}", &text[..at], color & 0x00FF_FFFF, &text[at + CHIP_TINT.len()..]).into_bytes(),
        _ => image.to_vec(),
    }
}

fn put_avatar_span(
    out: &mut Out,
    bytes: &[u8],
    chip: &Item,
    decoration: &AstralMentionDecoration,
    change: Change,
    properties: &AstralWireProperties,
) {
    let kept = kept_fields(bytes, chip, MAX_KEPT_CHIP_FIELDS, |key| {
        if (change.background != 0 && key == properties.bg) || (change.color != 0 && key == properties.color) {
            return false;
        }
        [
            properties.bg,
            properties.bg_radius,
            properties.bg_fit_text,
            properties.link,
            properties.color,
        ]
        .contains(&key)
    });
    out.put_tag(Tag::Map);
    out.put32(kept.len() as u32 + 2 + u32::from(change.background != 0) + u32::from(change.color != 0));
    for (start, end) in kept {
        out.put(&bytes[start..end]);
    }
    if change.background != 0 {
        out.prop_int(properties.bg, change.background as i32);
    }
    if change.color != 0 {
        put_long(out, properties.color, change.color);
    }
    out.prop_string(properties.text, b"avatar");
    out.put16(properties.image);
    out.put_tag(Tag::Map);
    out.put32(if decoration.round != 0 { 4 } else { 3 });
    let tint = if change.color != 0 { change.color } else { chip.color };
    out.prop_string(properties.uri, &chip_uri(decoration.image(), tint));
    out.prop_number(properties.size, 16.0);
    out.prop_string(properties.pool, b"small");
    if decoration.round != 0 {
        out.prop_number(properties.radius, 8.0);
    }
}

fn decorate_chips(bytes: &[u8], items: &[Item], message: u64, unresolved: &mut bool) -> (Vec<AstralMentionDecoration>, bool) {
    let decorator = DECORATOR.get::<MentionDecorator>();
    let mut decorations = vec![AstralMentionDecoration::default(); items.len()];
    let mut any = false;
    let mut mention = 0;
    for (index, item) in items.iter().enumerate() {
        let Some((at, length)) = item.text.filter(|_| item.chip) else {
            continue;
        };
        let text = CString::new(&bytes[at..at + length.min(MAX_TEXT - 1)]).unwrap_or_default();
        let context = AstralMentionContext {
            message_id: message,
            channel_id: current_channel(),
            text: text.as_ptr() as *const c_char,
            index: mention,
        };
        mention += 1;
        let decoration = &mut decorations[index];
        let decorated = decorator.is_some_and(|decorate| unsafe { decorate(&context, decoration) } != 0);
        if decorated && (!decoration.image().is_empty() || decoration.hide_at != 0 || decoration.color != 0) {
            any = true;
        } else {
            *unresolved = true;
        }
    }
    (decorations, any)
}

fn text_color(message: u64, unresolved: &mut bool) -> u32 {
    let Some(colorizer) = COLORIZER.get::<TextColorizer>() else {
        return 0;
    };
    let mut color = 0;
    if unsafe { colorizer(message, current_channel(), &mut color) } < 0 {
        *unresolved = true;
    }
    color
}

fn changes_for(items: &[Item], decorations: &[AstralMentionDecoration], text_color: u32) -> Vec<Change> {
    let mut changes = vec![Change::default(); items.len()];
    for (index, item) in items.iter().enumerate() {
        if !item.chip_part && item.color != 0 && text_color != 0 {
            changes[index].color = blended(item.color, text_color);
        }
        if !item.chip {
            continue;
        }
        changes[index].drop_at = decorations[index].hide_at != 0;
        let chip_color = decorations[index].color;
        if chip_color == 0 {
            continue;
        }
        let background = (chip_color & 0x00FF_FFFF) | 0x2600_0000;
        for part in index.saturating_sub(1)..=(index + 1).min(items.len() - 1) {
            if items[part].chip_part {
                changes[part].color = chip_color | 0xFF00_0000;
                changes[part].background = background;
            }
        }
    }
    changes
}

fn decorate_text(bytes: &[u8], span: &Span, message: u64, out: &mut Out, unresolved: &mut bool) -> bool {
    let Some(spans) = decorated_spans(bytes, span, message, unresolved) else {
        return false;
    };
    out.update(span.id, 1);
    out.put16(properties::current().spans);
    out.put(&spans);
    true
}

pub fn decorated_spans(bytes: &[u8], span: &Span, message: u64, unresolved: &mut bool) -> Option<Vec<u8>> {
    if !active() {
        return None;
    }
    let mut out = Out::default();
    let written = write_decorated_spans(bytes, span, message, &mut out, unresolved) && out.failed == 0;
    let spans = written.then(|| unsafe { core::slice::from_raw_parts(out.bytes, out.length) }.to_vec());
    out.discard();
    spans
}

fn write_decorated_spans(bytes: &[u8], span: &Span, message: u64, out: &mut Out, unresolved: &mut bool) -> bool {
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
        let Some(items) = read_items(&mut cursor, &properties) else {
            return false;
        };
        if items.is_empty() {
            return false;
        }
        let (decorations, any) = decorate_chips(bytes, &items, message, unresolved);
        let color = text_color(message, unresolved);
        if !any && color == 0 {
            return false;
        }
        let changes = changes_for(&items, &decorations, color);
        let added = decorations.iter().filter(|decoration| !decoration.image().is_empty()).count() as u32;
        out.put_tag(Tag::Array);
        out.put32(items.len() as u32 + added);
        for (index, item) in items.iter().enumerate() {
            let decoration = &decorations[index];
            let image = !decoration.image().is_empty();
            let chip = Change {
                drop_at: false,
                color: changes[index].color,
                background: changes[index].background,
            };
            if image && decoration.after == 0 {
                put_avatar_span(out, bytes, item, decoration, chip, &properties);
            }
            put_span(out, bytes, item, changes[index], &properties);
            if image && decoration.after != 0 {
                put_avatar_span(out, bytes, item, decoration, chip, &properties);
            }
        }
        return true;
    }
    false
}

fn content_message(bytes: *const u8, spans: &[Span], id: i32) -> Option<u64> {
    super::content_views::message_of(bytes, spans, id)
}

fn with_pending<T>(work: impl FnOnce(&mut HashMap<i32, Pending>) -> T) -> Option<T> {
    let mut pending = PENDING.lock().ok()?;
    Some(work(pending.get_or_insert_with(HashMap::new)))
}

fn remember(bytes: &[u8], span: &Span, message: u64) {
    with_pending(|pending| {
        if pending.len() < MAX_PENDING && !pending.contains_key(&span.id) {
            pending.insert(
                span.id,
                Pending {
                    message,
                    creation: bytes[span.start..span.end].to_vec(),
                    tries: 0,
                },
            );
        }
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_wire_pending_mentions() -> c_int {
    with_pending(|pending| pending.len() as c_int).unwrap_or(0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_wire_retry_mentions(buffer: *mut AstralWireBuffer) -> c_int {
    let Some(buffer) = (unsafe { buffer.as_mut() }) else { return 0 };
    *buffer = AstralWireBuffer {
        bytes: core::ptr::null_mut(),
        length: 0,
    };
    if !active() {
        return 0;
    }
    let mut out = Out::batch();
    let sent = with_pending(|pending| {
        let mut sent = false;
        pending.retain(|id, entry| {
            let span = Span {
                op: 1,
                id: *id,
                start: 0,
                end: entry.creation.len(),
                children: 0,
                child_count: 0,
            };
            let mut unresolved = false;
            if let Some(spans) = decorated_spans(&entry.creation, &span, entry.message, &mut unresolved) {
                if !super::message_text::adopt_spans(*id, entry.message, &spans, &mut out) {
                    out.update(*id, 1);
                    out.put16(properties::current().spans);
                    out.put(&spans);
                }
                sent = true;
                return false;
            }
            entry.tries += 1;
            entry.tries < MAX_RETRIES
        });
        sent
    })
    .unwrap_or(false);
    if !sent {
        out.discard();
        return 0;
    }
    c_int::from(out.finish(buffer))
}

#[unsafe(export_name = "wireDecorateMentions")]
pub unsafe extern "C" fn wire_decorate_mentions(
    bytes: *const u8,
    pointer: *const Span,
    count: usize,
    out: *mut Out,
    changed: *mut c_int,
    length: usize,
) {
    if !active() {
        return;
    }
    let (Some(out), Some(changed)) = (unsafe { out.as_mut() }, unsafe { changed.as_mut() }) else {
        return;
    };
    let spans = spans_of(pointer, count);
    super::content_views::observe(bytes, spans);
    for span in spans {
        let data = batch_of(bytes, span);
        if span.op == Op::Remove {
            with_pending(|pending| {
                for position in 0..span.child_count {
                    pending.remove(&child(data, span, position));
                }
            });
            continue;
        }
        if span.op != Op::Create || !type_is(data, span, b"text") {
            continue;
        }
        let Some(message) = content_message(bytes, spans, span.id) else {
            continue;
        };
        let mut update = Out::default();
        let mut unresolved = false;
        if decorate_text(data, span, message, &mut update, &mut unresolved) && update.failed == 0 {
            if *changed == 0 {
                out.put(unsafe { core::slice::from_raw_parts(bytes, length) });
            }
            *changed = 1;
            out.put(unsafe { core::slice::from_raw_parts(update.bytes, update.length) });
        } else if unresolved {
            remember(data, span, message);
        }
        update.discard();
    }
}
