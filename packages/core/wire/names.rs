use core::ffi::c_int;
use std::ffi::CString;
use std::sync::atomic::{AtomicI32, Ordering};

use super::abi::{
    AstralMentionDecoration, AstralNameContext, Callback, NAME_AVATAR, NAME_HEADER, NAME_TYPING, NAME_VOTER, avatar_user, current_channel,
    prefixed,
};
use super::cursor::Cursor;
use super::format::{Span, batch_of, child, find_string, property_cursor, spans_of, type_is, wireChildList, wireCreation, wireParentOf};
use super::out::Out;
use super::properties;

pub type NameColorizer = unsafe extern "C" fn(*const AstralNameContext, *mut u32) -> c_int;
pub type TypingDecorator = unsafe extern "C" fn(*const AstralNameContext, *mut AstralMentionDecoration) -> c_int;

const MAX_NAME_TEXT: usize = 128;
const MAX_NAME_SPANS: u32 = 16;
const MAX_KEPT_FIELDS: usize = 24;
const COUNT_SEPARATOR: &[u8] = " \u{2014} ".as_bytes();

static COLORIZER: Callback = Callback::new();
static TYPING: Callback = Callback::new();
static TYPING_NODE: AtomicI32 = AtomicI32::new(0);

#[unsafe(no_mangle)]
pub extern "C" fn astral_wire_set_name_colorizer(colorizer: Option<NameColorizer>) {
    COLORIZER.set(colorizer);
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_wire_set_typing_decorator(decorator: Option<TypingDecorator>) {
    TYPING.set(decorator);
}

struct Batch<'a> {
    bytes: *const u8,
    spans: &'a [Span],
}

impl Batch<'_> {
    fn parent(&self, id: i32) -> i32 {
        unsafe { wireParentOf(self.bytes, self.spans.as_ptr(), self.spans.len(), id) }
    }

    fn children(&self, id: i32) -> Option<(&Span, &[u8])> {
        let list = unsafe { wireChildList(self.bytes, self.spans.as_ptr(), self.spans.len(), id).as_ref() }?;
        Some((list, batch_of(self.bytes, list)))
    }

    fn creation(&self, id: i32) -> Option<(&Span, &[u8])> {
        let creation = unsafe { wireCreation(self.spans.as_ptr(), self.spans.len(), id).as_ref() }?;
        Some((creation, batch_of(self.bytes, creation)))
    }

    fn first_child_is(&self, parent: i32, id: i32) -> bool {
        self.children(parent)
            .is_some_and(|(list, data)| list.child_count > 0 && child(data, list, 0) == id)
    }
}

fn avatar_before(batch: &Batch, parent: i32, id: i32) -> u64 {
    let uri = properties::current().uri;
    let Some((list, data)) = (parent != 0).then(|| batch.children(parent)).flatten() else {
        return 0;
    };
    for position in 0..list.child_count {
        let sibling = child(data, list, position);
        if sibling == id {
            return 0;
        }
        if let Some((creation, bytes)) = batch.creation(sibling)
            && type_is(bytes, creation, b"image")
            && let Some(text) = find_string(bytes, creation, uri)
        {
            let user = avatar_user(text);
            if user != 0 {
                return user;
            }
        }
    }
    0
}

fn sibling_avatar(batch: &Batch, id: i32) -> u64 {
    let parent = batch.parent(id);
    let user = avatar_before(batch, parent, id);
    if user != 0 || parent == 0 || !batch.first_child_is(parent, id) {
        return user;
    }
    avatar_before(batch, batch.parent(parent), parent)
}

fn voter_row(batch: &Batch, id: i32) -> u64 {
    let parent = batch.parent(id);
    if parent == 0 || !batch.first_child_is(parent, id) {
        return 0;
    }
    let accessibility = properties::current().accessibilityId;
    let mut ancestor = parent;
    for _ in 0..3 {
        ancestor = batch.parent(ancestor);
        if ancestor == 0 {
            break;
        }
        if let Some((creation, bytes)) = batch.creation(ancestor)
            && let Some(identifier) = find_string(bytes, creation, accessibility)
            && let Some(user) = prefixed(identifier, c"voter-")
        {
            return user;
        }
    }
    0
}

fn is_header(bytes: &[u8], span: &Span, text: &[u8]) -> bool {
    find_string(bytes, span, properties::current().role) == Some(b"header")
        && text.windows(COUNT_SEPARATOR.len()).any(|window| window == COUNT_SEPARATOR)
}

fn number_property(bytes: &[u8], span: &Span, wanted: u16) -> f64 {
    let (mut cursor, count) = property_cursor(bytes, span);
    for _ in 0..count {
        if cursor.failed() {
            break;
        }
        let property = cursor.read16();
        let tag = cursor.read8();
        if property == wanted
            && tag == 5
            && let Some(value) = cursor.take(8)
        {
            return f64::from_le_bytes(value.try_into().unwrap_or([0; 8]));
        }
        cursor.skip_value(tag, 0);
    }
    0.0
}

fn is_typing_line(batch: &Batch, id: i32) -> bool {
    let properties = properties::current();
    let Some((list, data)) = batch.children(batch.parent(id)) else {
        return false;
    };
    let mut dots = 0;
    for position in 0..list.child_count {
        let sibling = child(data, list, position);
        if sibling == id {
            return dots >= 3;
        }
        if let Some((creation, bytes)) = batch.creation(sibling)
            && type_is(bytes, creation, b"view")
            && number_property(bytes, creation, properties.width) == 6.0
            && number_property(bytes, creation, properties.height) == 6.0
            && number_property(bytes, creation, properties.radius) >= 999.0
        {
            dots += 1;
        }
    }
    false
}

struct NameSpan {
    start: usize,
    end: usize,
    text: Option<(usize, usize)>,
    bold: bool,
}

fn read_name_spans(bytes: &[u8], span: &Span) -> Vec<NameSpan> {
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
        return name_entries(&mut cursor, properties.text, properties.weight).unwrap_or_default();
    }
    Vec::new()
}

fn name_entries(cursor: &mut Cursor, text_key: u16, weight_key: u16) -> Option<Vec<NameSpan>> {
    let total = cursor.read32();
    if total > MAX_NAME_SPANS {
        return None;
    }
    let mut items = Vec::new();
    for _ in 0..total {
        let start = cursor.offset();
        if cursor.read8() != 8 {
            return None;
        }
        let mut entry = NameSpan {
            start,
            end: 0,
            text: None,
            bold: false,
        };
        let fields = cursor.read32();
        for _ in 0..fields {
            if cursor.failed() {
                return None;
            }
            let key = cursor.read16();
            let tag = cursor.read8();
            if key == text_key && tag == 6 {
                let length = cursor.read32() as usize;
                let at = cursor.offset();
                cursor.take(length)?;
                entry.text = Some((at, length));
                continue;
            }
            if key == weight_key
                && tag == 3
                && let Some(weight) = cursor.bytes().get(cursor.offset()..cursor.offset() + 4)
            {
                entry.bold = i32::from_le_bytes(weight.try_into().ok()?) >= 600;
            }
            cursor.skip_value(tag, 1);
        }
        entry.end = cursor.offset();
        items.push(entry);
    }
    (!cursor.failed()).then_some(items)
}

fn put_colored_span(out: &mut Out, bytes: &[u8], item: &NameSpan, color: u32) {
    if color == 0 {
        out.put(&bytes[item.start..item.end]);
        return;
    }
    let color_key = properties::current().color;
    let mut cursor = Cursor::new(&bytes[..item.end], item.start + 1);
    let entries = cursor.read32();
    let mut kept = Vec::new();
    for _ in 0..entries {
        if cursor.failed() || kept.len() >= MAX_KEPT_FIELDS {
            break;
        }
        let at = cursor.offset();
        let key = cursor.read16();
        let tag = cursor.read8();
        cursor.skip_value(tag, 1);
        if key != color_key {
            kept.push((at, cursor.offset()));
        }
    }
    out.put8(8);
    out.put32(kept.len() as u32 + 1);
    for (start, end) in kept {
        out.put(&bytes[start..end]);
    }
    out.put16(color_key);
    out.put8(4);
    out.put(&i64::from(color).to_le_bytes());
}

fn put_typing_avatar(out: &mut Out, image: &[u8], round: bool) {
    let properties = properties::current();
    out.put8(8);
    out.put32(2);
    out.prop_string(properties.text, b"avatar");
    out.put16(properties.image);
    out.put8(8);
    out.put32(if round { 4 } else { 3 });
    out.prop_string(properties.uri, image);
    out.prop_number(properties.size, 14.0);
    out.prop_string(properties.pool, b"small");
    if round {
        out.prop_number(properties.radius, 7.0);
    }
}

fn put_spacer(out: &mut Out) {
    out.put8(8);
    out.put32(1);
    out.prop_string(properties::current().text, b" ");
}

fn name_context(kind: c_int, name: &CString, user: u64) -> AstralNameContext {
    AstralNameContext {
        kind,
        text: name.as_ptr(),
        user_id: user,
        channel_id: current_channel(),
    }
}

fn color_typing(bytes: &[u8], span: &Span, out: &mut Out) -> bool {
    let items = read_name_spans(bytes, span);
    let colorizer = COLORIZER.get::<NameColorizer>();
    let typing = TYPING.get::<TypingDecorator>();
    let mut colors = vec![0u32; items.len()];
    let mut avatars = vec![AstralMentionDecoration::default(); items.len()];
    let mut any = false;
    let mut added = 0;
    for (index, item) in items.iter().enumerate() {
        let Some((at, length)) = item.text else { continue };
        if !item.bold || length == 0 || length >= MAX_NAME_TEXT {
            continue;
        }
        let Ok(name) = CString::new(&bytes[at..at + length]) else {
            continue;
        };
        let context = name_context(NAME_TYPING, &name, 0);
        if let Some(colorizer) = colorizer
            && unsafe { colorizer(&context, &mut colors[index]) } != 0
            && colors[index] != 0
        {
            any = true;
        }
        if let Some(typing) = typing
            && unsafe { typing(&context, &mut avatars[index]) } != 0
            && !avatars[index].image().is_empty()
        {
            any = true;
            added += 2;
        }
    }
    if !any {
        return false;
    }
    out.update(span.id, 1);
    out.put16(properties::current().spans);
    out.put8(7);
    out.put32(items.len() as u32 + added);
    for (index, item) in items.iter().enumerate() {
        if !avatars[index].image().is_empty() {
            put_typing_avatar(out, avatars[index].image(), avatars[index].round != 0);
            put_spacer(out);
        }
        put_colored_span(out, bytes, item, colors[index]);
    }
    true
}

fn name_color(batch: &Batch, bytes: &[u8], span: &Span) -> Option<u32> {
    let colorizer = COLORIZER.get::<NameColorizer>()?;
    if span.op != 1 || !type_is(bytes, span, b"text") {
        return None;
    }
    let text = find_string(bytes, span, properties::current().text)?;
    if text.is_empty() || text.len() >= MAX_NAME_TEXT {
        return None;
    }
    let (kind, user) = if is_header(bytes, span, text) {
        (NAME_HEADER, 0)
    } else {
        match sibling_avatar(batch, span.id) {
            0 => (NAME_VOTER, voter_row(batch, span.id)),
            user => (NAME_AVATAR, user),
        }
    };
    if kind != NAME_HEADER && user == 0 {
        return None;
    }
    let name = CString::new(text).ok()?;
    let context = name_context(kind, &name, user);
    let mut color = 0;
    (unsafe { colorizer(&context, &mut color) } != 0 && color != 0).then_some(color)
}

fn put_color(out: &mut Out, id: i32, color: u32) {
    out.update(id, 1);
    out.put16(properties::current().color);
    out.put8(4);
    out.put(&i64::from(color).to_le_bytes());
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wireColorNames(
    bytes: *const u8,
    pointer: *const Span,
    count: usize,
    out: *mut Out,
    changed: *mut c_int,
    length: usize,
) {
    if COLORIZER.get::<NameColorizer>().is_none() && TYPING.get::<TypingDecorator>().is_none() {
        return;
    }
    let (Some(out), Some(changed)) = (unsafe { out.as_mut() }, unsafe { changed.as_mut() }) else {
        return;
    };
    let batch = Batch {
        bytes,
        spans: spans_of(pointer, count),
    };
    let original = || unsafe { core::slice::from_raw_parts(bytes, length) };
    for span in batch.spans {
        let data = batch_of(bytes, span);
        let typing_node = TYPING_NODE.load(Ordering::Relaxed);
        let mut typing = span.op == 2 && typing_node != 0 && span.id == typing_node;
        if !typing && span.op == 1 && type_is(data, span, b"text") && is_typing_line(&batch, span.id) {
            TYPING_NODE.store(span.id, Ordering::Relaxed);
            typing = true;
        }
        if typing {
            let mut update = Out::default();
            if color_typing(data, span, &mut update) && update.failed == 0 {
                if *changed == 0 {
                    out.put(original());
                }
                *changed = 1;
                out.put(unsafe { core::slice::from_raw_parts(update.bytes, update.length) });
            }
            update.discard();
            continue;
        }
        let Some(color) = name_color(&batch, data, span) else { continue };
        if *changed == 0 {
            out.put(original());
        }
        *changed = 1;
        put_color(out, span.id, color);
    }
}
