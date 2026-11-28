use super::tag::{Op, Tag};
use crate::lua::enums::{BubbleGrouping, ReplyPlacement, TimePlacement};
use core::ffi::{c_char, c_int};
use std::collections::{HashMap, HashSet};
use std::os::unix::ffi::OsStrExt;
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

use super::abi::{Callback, Tracked};
use super::children::discord_children;
use super::cursor::Cursor;
use super::format::{Span, child, find_string, property_cursor, spans_of, type_is, wire_child_list, wire_creation};
use super::ids::{avatar_user, current_channel, prefixed, wire_set_current_channel};
use super::insert::{INSERT_CAPACITY, wire_inserted_children};
use super::message_text::wire_apply_message_text;
use super::out::{AstralWireBuffer, Out};
use super::patch::{message_of_identifier, read_identifier, wire_mark_refresh_busy, wire_patching_active};
use super::properties;

const OPAQUE_ALPHA: u32 = 0xFF;
const LUMA_RED: u32 = 299;
const LUMA_GREEN: u32 = 587;
const LUMA_BLUE: u32 = 114;
const LUMA_SCALE: u32 = 1000;
const DARK_LUMA: u32 = 64;

pub const TRACK_NONE: u8 = 0;
pub const TRACK_AUTHOR: u8 = 1;
pub const TRACK_AVATAR: u8 = 2;
pub const TRACK_AVATAR_IMAGE: u8 = 3;
pub const TRACK_CHANNEL: u8 = 4;
pub const TRACK_TIMESTAMP: u8 = 5;

const TRACK_CAPACITY: usize = 32768;
const MAX_TAGS: usize = 4;
const MAX_DECORATED: usize = 4096;
const MAX_TAGGED_CHILDREN: usize = 64;
const MERGED_CHILDREN: usize = 68;
const FIRST_TAG_ID: i32 = 0x7b00_0000;
const TAG_ID_RANGE: i32 = 0x00f0_0000;

const ROW_CAPACITY: usize = 512;
const MEDIA_CAPACITY: usize = 512;
const INSIDE_CAPACITY: usize = 256;
const BUBBLE_CHILDREN: usize = 4;
const BUBBLE_BLOCKS: usize = 6;
const TIME_TEXT_CAPACITY: usize = 48;

const ICON_TAG_SIZE: f64 = 16.0;
const CONTENT_PREFIX: &[u8] = b"message.content:";
const BUBBLE_PADDING: [f32; 4] = [14.0, 10.0, 14.0, 10.0];
const MEDIA_BLOCK_PADDING: [f32; 4] = [0.0, 8.0, 0.0, 0.0];
const MEDIA_INSET: f32 = 4.0;
const REPLY_PADDING: [f32; 4] = [8.0, 3.0, 8.0, 3.0];
const NO_PADDING: [f32; 4] = [0.0; 4];
const AVATAR_DECORATION_INSET: f32 = 4.0;
const SQUARED_CORNER_RATIO: f64 = 0.3;
const CORNER_TOP_LEFT: i32 = 1;
const CORNER_TOP_RIGHT: i32 = 2;
const CORNER_BOTTOM_LEFT: i32 = 4;
const CORNER_BOTTOM_RIGHT: i32 = 8;
const CORNERS_ALL: i32 = 15;
const GROUP_GAP: f32 = 4.0;
const REPLY_RADIUS: f64 = 9.0;
const QUOTE_PADDING: [f32; 4] = [8.0, 4.0, 8.0, 4.0];
const QUOTE_COLOR: i64 = 0x2600_0000;

const TAG_ORIGINAL_POSTER: c_int = 1;
const TAG_ICON: c_int = 2;
const TAG_TEXT: c_int = 3;

const ICON_CACHE_SIZE: usize = 8;
const MAX_ICON_FILE_BYTES: usize = 64 * 1024;
const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AstralTag {
    pub text: [c_char; 32],
    pub action_id: [c_char; 96],
    pub background: u32,
    pub foreground: u32,
    pub gradient: [u32; 3],
    pub gradient_count: u32,
    pub shape: c_int,
    pub icon: [c_char; 512],
    pub tint: u32,
}

impl AstralTag {
    pub const fn zeroed() -> Self {
        AstralTag {
            text: [0; 32],
            action_id: [0; 96],
            background: 0,
            foreground: 0,
            gradient: [0; 3],
            gradient_count: 0,
            shape: 0,
            icon: [0; 512],
            tint: 0,
        }
    }
}

#[repr(C)]
pub struct Decorated {
    pub parent: i32,
    pub tags: [i32; MAX_TAGS * 2],
    pub count: u32,
    pub roots: [i32; MAX_TAGS],
    pub root_count: u32,
    pub message: u64,
    pub signature: u64,
}

#[repr(C)]
pub struct AstralDecorationContext {
    pub prefix: *const c_char,
    pub message_id: u64,
    pub user_id: u64,
    pub channel_id: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct AstralWireStats {
    pub authors: u64,
    pub with_user: u64,
    pub with_channel: u64,
    pub tagged: u64,
}

#[repr(C)]
pub struct AstralMessageBackgroundContext {
    pub message_id: u64,
    pub user_id: u64,
    pub channel_id: u64,
}

#[repr(C)]
#[derive(Default)]
pub struct AstralMessageBackgroundStyle {
    pub color: u32,
    pub radius: f64,
    pub bubble: c_int,
    pub reply: ReplyPlacement,
    pub name_inside: c_int,
    pub time: TimePlacement,
    pub padding_x: f64,
    pub padding_y: f64,
    pub grouping: BubbleGrouping,
}

pub type AstralDecorator = unsafe extern "C" fn(*const AstralDecorationContext, *mut AstralTag, usize) -> usize;
pub type AstralMessageBackgroundDecorator =
    unsafe extern "C" fn(*const AstralMessageBackgroundContext, *mut AstralMessageBackgroundStyle) -> c_int;

static DECORATOR: Callback = Callback::new();
static BACKGROUND_DECORATOR: Callback = Callback::new();
static NEXT_TAG_ID: AtomicI32 = AtomicI32::new(FIRST_TAG_ID);

static STAT_AUTHORS: AtomicU64 = AtomicU64::new(0);
static STAT_WITH_USER: AtomicU64 = AtomicU64::new(0);
static STAT_WITH_CHANNEL: AtomicU64 = AtomicU64::new(0);
static STAT_TAGGED: AtomicU64 = AtomicU64::new(0);

#[derive(Default)]
struct Tags {
    nodes: Vec<i32>,
    roots: Vec<i32>,
    trailing: Vec<i32>,
    message: u64,
    signature: u64,
}

#[derive(Clone, Default)]
struct Block {
    id: i32,
    media: bool,
    card: i32,
    card_color: u32,
    card_radius: f64,
    accent: i32,
    accent_color: u32,
    padding: [f32; 4],
    children: Vec<i32>,
    wide: bool,
}

#[derive(Default)]
struct Row {
    id: i32,
    message: u64,
    user: u64,
    channel: u64,
    original_color: u32,
    applied_color: u32,
    original_radius: f64,
    applied_radius: f64,
    applied_padding: [f32; 4],
    has_original: bool,
    active: bool,
    bubble: bool,
    reply_shown: ReplyPlacement,
    column: i32,
    reply: i32,
    reply_bar: i32,
    bar_children: Vec<i32>,
    bar_padding: [f32; 4],
    bar_margin: [f32; 4],
    quoted_in: i32,
    quote_box: i32,
    header: i32,
    timestamp: i32,
    time_parent: i32,
    time_parent_children: Vec<i32>,
    time_text: Vec<u8>,
    column_children: Vec<i32>,
    name_shown: bool,
    avatar: i32,
    avatar_margin: [f32; 4],
    avatar_shifted: bool,
    corners: i32,
    grouping: BubbleGrouping,
    group_start: bool,
    group_end: bool,
    inner: i32,
    inner_padding: [f32; 4],
    group_gap: Option<f32>,
    time_shown: TimePlacement,
    blocks: Vec<Block>,
    shown_blocks: usize,
}

#[derive(Default)]
struct Backgrounds {
    rows: HashMap<i32, Row>,
    inside: Vec<i32>,
}

#[derive(Clone, Copy, Default)]
struct ListedRow {
    id: i32,
    message: u64,
    user: u64,
    content: i32,
}

type Icons = Vec<(Vec<u8>, Vec<u8>)>;
type InlineTimes = HashMap<u64, (i32, Vec<u8>)>;

static TRACKED: Mutex<Option<HashMap<i32, Tracked>>> = Mutex::new(None);
static AUTHORS: Mutex<Vec<(u64, u64)>> = Mutex::new(Vec::new());
static DECORATED: Mutex<Option<HashMap<i32, Tags>>> = Mutex::new(None);
static BACKGROUNDS: Mutex<Option<Backgrounds>> = Mutex::new(None);
static INLINE_TIMES: Mutex<Option<InlineTimes>> = Mutex::new(None);
static ENSURED_TIMES: Mutex<Option<HashMap<u64, Vec<u8>>>> = Mutex::new(None);
static ICONS: Mutex<(Icons, usize)> = Mutex::new((Vec::new(), 0));

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn with_tracked<T>(work: impl FnOnce(&mut HashMap<i32, Tracked>) -> T) -> T {
    let mut guard = lock(&TRACKED);
    work(guard.get_or_insert_with(HashMap::new))
}

fn with_decorated<T>(work: impl FnOnce(&mut HashMap<i32, Tags>) -> T) -> T {
    let mut guard = lock(&DECORATED);
    work(guard.get_or_insert_with(HashMap::new))
}

fn with_backgrounds<T>(work: impl FnOnce(&mut Backgrounds) -> T) -> T {
    let mut guard = lock(&BACKGROUNDS);
    work(guard.get_or_insert_with(Backgrounds::default))
}

fn with_inline_times<T>(work: impl FnOnce(&mut InlineTimes) -> T) -> T {
    let mut guard = lock(&INLINE_TIMES);
    work(guard.get_or_insert_with(HashMap::new))
}

pub fn tracked(id: i32) -> Option<Tracked> {
    with_tracked(|tracked| tracked.get(&id).copied()).filter(|slot| slot.kind != TRACK_NONE)
}

pub fn tracked_value(id: i32, kind: u8) -> Option<u64> {
    tracked(id).filter(|slot| slot.kind == kind).map(|slot| slot.value)
}

fn track(tracked: &mut HashMap<i32, Tracked>, id: i32, kind: u8, value: u64) {
    if tracked.len() < TRACK_CAPACITY || tracked.contains_key(&id) {
        tracked.insert(id, Tracked { id, kind, value });
    }
}

fn kind_of(tracked: &HashMap<i32, Tracked>, id: i32) -> u8 {
    tracked.get(&id).map_or(TRACK_NONE, |slot| slot.kind)
}

fn author_slot(message: u64) -> usize {
    (message % TRACK_CAPACITY as u64) as usize
}

fn remember_author(message: u64, user: u64) {
    let mut authors = lock(&AUTHORS);
    if authors.is_empty() {
        authors.resize(TRACK_CAPACITY, (0, 0));
    }
    authors[author_slot(message)] = (message, user);
}

pub fn author_of(message: u64) -> u64 {
    if message == 0 {
        return 0;
    }
    let authors = lock(&AUTHORS);
    authors
        .get(author_slot(message))
        .filter(|entry| entry.0 == message)
        .map_or(0, |entry| entry.1)
}

#[unsafe(export_name = "wireMessageAuthor")]
pub extern "C" fn wire_message_author(message: u64) -> u64 {
    author_of(message)
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_wire_set_decorator(decorator: Option<AstralDecorator>) {
    DECORATOR.set(decorator);
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_wire_set_message_background_decorator(decorator: Option<AstralMessageBackgroundDecorator>) {
    BACKGROUND_DECORATOR.set(decorator);
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_wire_stats() -> AstralWireStats {
    AstralWireStats {
        authors: STAT_AUTHORS.load(Ordering::Relaxed),
        with_user: STAT_WITH_USER.load(Ordering::Relaxed),
        with_channel: STAT_WITH_CHANNEL.load(Ordering::Relaxed),
        tagged: STAT_TAGGED.load(Ordering::Relaxed),
    }
}

fn tag_id() -> i32 {
    NEXT_TAG_ID
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| {
            Some(if id + 1 >= FIRST_TAG_ID + TAG_ID_RANGE {
                FIRST_TAG_ID
            } else {
                id + 1
            })
        })
        .unwrap_or(FIRST_TAG_ID)
}

#[unsafe(export_name = "wireAllocateNode")]
pub extern "C" fn wire_allocate_node() -> i32 {
    tag_id()
}

#[unsafe(export_name = "wireRowMessage")]
pub extern "C" fn wire_row_message(row: i32) -> u64 {
    with_backgrounds(|backgrounds| {
        backgrounds
            .rows
            .get(&row)
            .filter(|state| state.has_original)
            .map_or(0, |state| state.message)
    })
}

fn field(text: &[c_char]) -> &[u8] {
    let length = text.iter().position(|byte| *byte == 0).unwrap_or(text.len());
    unsafe { core::slice::from_raw_parts(text.as_ptr().cast::<u8>(), length) }
}

fn put_original_once(out: &mut Out, original: &[u8], changed: &mut c_int) {
    if *changed == 0 {
        out.put(original);
    }
    *changed = 1;
}

struct Batch<'a> {
    bytes: &'a [u8],
    spans: &'a [Span],
}

impl<'a> Batch<'a> {
    fn creation(&self, id: i32) -> Option<&'a Span> {
        unsafe { wire_creation(self.spans.as_ptr(), self.spans.len(), id).as_ref() }
    }

    fn list(&self, id: i32) -> Option<&'a Span> {
        unsafe { wire_child_list(self.bytes.as_ptr(), self.spans.as_ptr(), self.spans.len(), id).as_ref() }
    }

    fn child(&self, span: &Span, position: u32) -> i32 {
        child(self.bytes, span, position)
    }

    fn children(&self, span: &Span, limit: usize) -> Vec<i32> {
        (0..span.child_count)
            .take(limit)
            .map(|position| self.child(span, position))
            .collect()
    }

    fn child_of(&self, parent: i32, position: u32) -> i32 {
        self.list(parent)
            .filter(|list| position < list.child_count)
            .map_or(0, |list| self.child(list, position))
    }

    fn was_removed(&self, id: i32) -> bool {
        self.spans
            .iter()
            .filter(|span| span.op == Op::Remove)
            .any(|span| (0..span.child_count).any(|position| self.child(span, position) == id))
    }
}

fn property_value<'a>(bytes: &'a [u8], span: &Span, wanted: u16, accepts: impl Fn(u8) -> bool) -> Option<Cursor<'a>> {
    let (mut cursor, count) = property_cursor(bytes, span);
    for _ in 0..count {
        if cursor.failed() {
            break;
        }
        let property = cursor.read16();
        let tag = cursor.read8();
        if property == wanted && accepts(tag) {
            return Some(cursor);
        }
        cursor.skip_value(tag, 0);
    }
    None
}

fn number_property(bytes: &[u8], span: &Span, wanted: u16) -> Option<f64> {
    let mut cursor = property_value(bytes, span, wanted, |tag| tag == Tag::Double)?;
    Some(f64::from_le_bytes(cursor.take(8)?.try_into().ok()?))
}

fn quad_property(bytes: &[u8], span: &Span, wanted: u16) -> Option<[f32; 4]> {
    let mut cursor = property_value(bytes, span, wanted, |tag| tag == Tag::Quad)?;
    let data = cursor.take(16)?;
    let mut quad = [0.0; 4];
    let (chunks, _) = data.as_chunks::<4>();
    for (value, chunk) in quad.iter_mut().zip(chunks) {
        *value = f32::from_le_bytes(*chunk);
    }
    Some(quad)
}

fn color_property(bytes: &[u8], span: &Span, wanted: u16) -> Option<u32> {
    let mut cursor = property_value(bytes, span, wanted, |tag| tag == Tag::Int || tag == Tag::Long)?;
    let value = cursor.read32();
    (!cursor.failed()).then_some(value)
}

fn string_is(bytes: &[u8], span: &Span, wanted: u16, value: &[u8]) -> bool {
    find_string(bytes, span, wanted) == Some(value)
}

fn observe_properties(bytes: &[u8], span: &Span) {
    let properties = properties::current();
    let (mut cursor, count) = property_cursor(bytes, span);
    let mut selected = None;
    for _ in 0..count {
        if cursor.failed() {
            break;
        }
        let property = cursor.read16();
        let tag = cursor.read8();
        if tag == Tag::Null && property == properties.accessibility_id && span.op == Op::Update {
            with_tracked(|tracked| tracked.remove(&span.id));
            continue;
        }
        if tag == Tag::String && (property == properties.accessibility_id || property == properties.uri) {
            let length = cursor.read32() as usize;
            let Some(text) = cursor.take(length) else { return };
            if let Some((kind, value)) = observed_kind(property == properties.accessibility_id, text) {
                with_tracked(|tracked| track(tracked, span.id, kind, value));
            }
            continue;
        }
        if property == properties.accessibility_selected && (tag == Tag::False || tag == Tag::True) {
            selected = Some(tag == Tag::True);
            continue;
        }
        cursor.skip_value(tag, 0);
    }
    let Some(selected) = selected else { return };
    let Some(channel) = tracked_value(span.id, TRACK_CHANNEL) else {
        return;
    };
    if selected {
        wire_set_current_channel(channel);
    } else if current_channel() == channel {
        wire_set_current_channel(0);
    }
}

fn observed_kind(accessibility: bool, text: &[u8]) -> Option<(u8, u64)> {
    if !accessibility {
        let user = avatar_user(text);
        return (user != 0).then_some((TRACK_AVATAR_IMAGE, user));
    }
    [
        (b"message.author:".as_slice(), TRACK_AUTHOR),
        (b"message.avatar:", TRACK_AVATAR),
        (b"message.timestamp:", TRACK_TIMESTAMP),
        (b"navigation.channel:", TRACK_CHANNEL),
    ]
    .into_iter()
    .find_map(|(prefix, kind)| prefixed(text, prefix).map(|value| (kind, value)))
}

fn propagate_avatars(batch: &Batch) {
    with_tracked(|tracked| {
        for _ in 0..3 {
            for span in batch.spans {
                if span.op != Op::SetChildren || span.child_count == 0 || span.child_count > 4 {
                    continue;
                }
                for position in 0..span.child_count {
                    let Some(image) = tracked.get(&batch.child(span, position)).copied() else {
                        continue;
                    };
                    if image.kind != TRACK_AVATAR_IMAGE {
                        continue;
                    }
                    let holder = tracked.get(&span.id).copied();
                    match holder.map_or(TRACK_NONE, |holder| holder.kind) {
                        TRACK_AVATAR => remember_author(holder.map_or(0, |holder| holder.value), image.value),
                        TRACK_NONE | TRACK_AVATAR_IMAGE => track(tracked, span.id, TRACK_AVATAR_IMAGE, image.value),
                        _ => {}
                    }
                    break;
                }
            }
        }
    });
}

fn remove_tags(out: &mut Out, slot: &mut Tags) {
    if slot.nodes.is_empty() {
        return;
    }
    out.remove(&slot.nodes);
    slot.nodes.clear();
    slot.roots.clear();
    slot.trailing.clear();
}

fn valid_style(style: &AstralMessageBackgroundStyle) -> bool {
    style.radius.is_finite() && (0.0..=32.0).contains(&style.radius)
}

fn put_unset_width(out: &mut Out, node: i32, fill: bool) {
    let properties = properties::current();
    out.update(node, 1);
    if fill {
        out.prop_string(properties.width, b"fill");
    } else {
        out.prop_unset(properties.width);
    }
}

fn write_card_update(out: &mut Out, block: &Block, shown: bool, color: u32) {
    let properties = properties::current();
    out.update(block.card, 2);
    out.prop_long(properties.background, i64::from(if shown { color } else { block.card_color }));
    out.prop_number(properties.radius, block.card_radius);
    if block.accent == 0 {
        return;
    }
    out.update(block.accent, 1);
    out.prop_long(properties.background, if shown { 0 } else { i64::from(block.accent_color) });
}

fn bubble_padding(style: &AstralMessageBackgroundStyle) -> [f32; 4] {
    let x = if style.padding_x > 0.0 {
        style.padding_x as f32
    } else {
        BUBBLE_PADDING[0]
    };
    let y = if style.padding_y > 0.0 {
        style.padding_y as f32
    } else {
        BUBBLE_PADDING[1]
    };
    [x, y, x, y]
}

fn write_block_update(out: &mut Out, block: &Block, shown: bool, color: u32, radius: f64, bubble_padding: [f32; 4]) {
    if block.card != 0 {
        write_card_update(out, block, shown, color);
        return;
    }
    let properties = properties::current();
    let padding = if block.media { block.padding } else { NO_PADDING };
    out.update(block.id, if block.media { 5 } else { 4 });
    out.prop_long(properties.background, if shown { i64::from(color) } else { 0 });
    out.prop_number(properties.radius, if shown { radius } else { 0.0 });
    let inner = match (shown, block.media) {
        (false, _) => padding,
        (true, true) => [MEDIA_INSET; 4],
        (true, false) => bubble_padding,
    };
    out.prop_quad(properties.padding, inner);
    if block.media {
        let top = if shown { block.padding[1] - MEDIA_INSET } else { 0.0 };
        out.prop_quad(properties.margin, [0.0, top, 0.0, 0.0]);
    }
    let fill = !shown || block.wide;
    if fill {
        out.prop_string(properties.width, b"fill");
    } else {
        out.prop_unset(properties.width);
    }
    for node in &block.children {
        put_unset_width(out, *node, fill);
    }
}

fn put_corner(out: &mut Out, property: u16, shown: bool, rounded: bool, radius: f64) {
    if !shown {
        out.prop_unset(property);
        return;
    }
    out.prop_number(property, if rounded { radius } else { radius * SQUARED_CORNER_RATIO });
}

fn write_corners(out: &mut Out, state: &Row, shown: bool, radius: f64) {
    let properties = properties::current();
    let corners = if state.corners >= 0 { state.corners } else { CORNERS_ALL };
    for block in state.blocks.iter().filter(|block| block.card == 0) {
        out.update(block.id, 4);
        put_corner(out, properties.radius_top_left, shown, corners & CORNER_TOP_LEFT != 0, radius);
        put_corner(out, properties.radius_top_right, shown, corners & CORNER_TOP_RIGHT != 0, radius);
        put_corner(out, properties.radius_bottom_left, shown, corners & CORNER_BOTTOM_LEFT != 0, radius);
        put_corner(
            out,
            properties.radius_bottom_right,
            shown,
            corners & CORNER_BOTTOM_RIGHT != 0,
            radius,
        );
    }
}

fn write_bubble_update(out: &mut Out, state: &Row, shown: bool, color: u32, radius: f64) {
    for block in &state.blocks {
        write_block_update(out, block, shown, color, radius, state.applied_padding);
    }
    write_corners(out, state, shown, radius);
}

fn write_preview_style(out: &mut Out, preview: i32, mode: ReplyPlacement, color: u32) {
    let properties = properties::current();
    let bubble = mode == ReplyPlacement::Bubble;
    out.update(preview, 3);
    out.prop_long(properties.background, if bubble { i64::from(color) } else { 0 });
    out.prop_number(properties.radius, if bubble { REPLY_RADIUS } else { 0.0 });
    out.prop_quad(properties.padding, if bubble { REPLY_PADDING } else { NO_PADDING });
}

fn write_background_update(out: &mut Out, row: i32, color: u32, radius: f64) {
    let properties = properties::current();
    out.update(row, 2);
    out.prop_long(properties.background, i64::from(color));
    out.prop_number(properties.radius, radius);
}

fn parent_in(links: &HashMap<i32, i32>, id: i32) -> i32 {
    if id == 0 {
        return 0;
    }
    links.get(&id).copied().unwrap_or(0)
}

fn known_parent(links: &HashMap<i32, i32>, id: i32) -> i32 {
    match parent_in(links, id) {
        0 if id != 0 => super::children::wire_discord_parent(id),
        parent => parent,
    }
}

fn ancestor_row(rows: &HashMap<i32, Row>, links: &HashMap<i32, i32>, block: i32) -> Option<i32> {
    let mut node = known_parent(links, block);
    for _ in 0..24 {
        if node == 0 {
            return None;
        }
        if rows.contains_key(&node) {
            return Some(node);
        }
        if let Some(row) = row_for_column(rows, node) {
            return Some(row);
        }
        node = known_parent(links, node);
    }
    None
}

fn row_for_node(links: &HashMap<i32, i32>, rows: &HashSet<i32>, mut id: i32) -> i32 {
    for _ in 0..24 {
        if id == 0 {
            break;
        }
        if rows.contains(&id) {
            return id;
        }
        id = known_parent_of(links, id);
    }
    0
}

fn capture_original(batch: &Batch, creation: Option<&Span>, state: &mut Row) {
    let properties = properties::current();
    state.original_color = creation
        .and_then(|creation| color_property(batch.bytes, creation, properties.background))
        .unwrap_or(0);
    state.original_radius = creation
        .and_then(|creation| number_property(batch.bytes, creation, properties.radius))
        .unwrap_or(0.0);
    state.inner = batch.child_of(state.id, 0);
    state.inner_padding = batch
        .creation(state.inner)
        .and_then(|inner| quad_property(batch.bytes, inner, properties.padding))
        .unwrap_or(NO_PADDING);
    state.has_original = true;
}

fn grouped_corners(state: &Row) -> i32 {
    let (start, end) = (state.group_start, state.group_end);
    match state.grouping {
        BubbleGrouping::Separate => CORNERS_ALL,
        BubbleGrouping::Grouped => {
            CORNER_TOP_RIGHT | CORNER_BOTTOM_RIGHT | if start { CORNER_TOP_LEFT } else { 0 } | if end { CORNER_BOTTOM_LEFT } else { 0 }
        }
        BubbleGrouping::Joined => {
            (if start { CORNER_TOP_LEFT | CORNER_TOP_RIGHT } else { 0 }) | if end { CORNER_BOTTOM_LEFT | CORNER_BOTTOM_RIGHT } else { 0 }
        }
    }
}

fn wanted_group_gap(state: &Row) -> Option<f32> {
    if !state.bubble || state.group_end {
        return None;
    }
    Some(if state.grouping == BubbleGrouping::Joined { 0.0 } else { GROUP_GAP })
}

fn write_group_gap(out: &mut Out, state: &mut Row) {
    let wanted = wanted_group_gap(state);
    if state.inner == 0 || wanted == state.group_gap {
        return;
    }
    let padding = state.inner_padding;
    out.update(state.inner, 1);
    out.prop_quad(
        properties::current().padding,
        wanted.map_or(padding, |gap| [padding[0], padding[1], padding[2], gap]),
    );
    state.group_gap = wanted;
}

fn is_media_block(batch: &Batch, span: &Span) -> bool {
    let properties = properties::current();
    span.op == Op::Create
        && type_is(batch.bytes, span, b"view")
        && string_is(batch.bytes, span, properties.width, b"fill")
        && quad_property(batch.bytes, span, properties.padding)
            .is_some_and(|padding| padding[0] == 0.0 && padding[2] == 0.0 && padding[3] == 0.0 && padding[1] > 0.0 && padding[1] <= 12.0)
}

fn add_block(state: &mut Row, id: i32, media: bool) -> Option<usize> {
    if let Some(index) = state.blocks.iter().position(|block| block.id == id) {
        return Some(index);
    }
    if state.blocks.len() >= BUBBLE_BLOCKS {
        return None;
    }
    state.blocks.push(Block {
        id,
        media,
        padding: MEDIA_BLOCK_PADDING,
        ..Block::default()
    });
    Some(state.blocks.len() - 1)
}

fn is_dark_opaque(color: u32) -> bool {
    let red = (color >> 16) & 0xFF;
    let green = (color >> 8) & 0xFF;
    let blue = color & 0xFF;
    color >> 24 == OPAQUE_ALPHA && red * LUMA_RED + green * LUMA_GREEN + blue * LUMA_BLUE < DARK_LUMA * LUMA_SCALE
}

fn remember_default_accent(batch: &Batch, block: &mut Block) {
    let properties = properties::current();
    block.accent = 0;
    let mut node = block.card;
    for _ in 0..2 {
        node = batch.child_of(node, 0);
        let Some(creation) = batch.creation(node) else { return };
        match number_property(batch.bytes, creation, properties.width) {
            Some(width) if width <= 6.0 => {}
            _ => continue,
        }
        let Some(color) = color_property(batch.bytes, creation, properties.background).filter(|color| is_dark_opaque(*color)) else {
            return;
        };
        block.accent = node;
        block.accent_color = color;
        return;
    }
}

fn remember_block_children(batch: &Batch, block: &mut Block) {
    let properties = properties::current();
    let Some(list) = batch.list(block.id) else { return };
    block.children.clear();
    block.wide = false;
    for position in 0..list.child_count {
        if block.children.len() >= BUBBLE_CHILDREN {
            break;
        }
        let node = batch.child(list, position);
        let creation = batch.creation(node);
        let card_color = creation.and_then(|creation| color_property(batch.bytes, creation, properties.background));
        if block.media
            && position == 0
            && let Some(card_color) = card_color.filter(|color| *color != 0)
        {
            block.card = node;
            block.card_color = card_color;
            block.card_radius = creation
                .and_then(|creation| number_property(batch.bytes, creation, properties.radius))
                .unwrap_or(0.0);
            remember_default_accent(batch, block);
            return;
        }
        if block.media && !creation.is_some_and(|creation| string_is(batch.bytes, creation, properties.width, b"fill")) {
            continue;
        }
        block.wide |= !block.media && holds_code(batch, node);
        block.children.push(node);
    }
}

fn holds_code(batch: &Batch, node: i32) -> bool {
    let is_code = |id: i32| {
        batch
            .creation(id)
            .is_some_and(|creation| super::code_blocks::is_code_block(batch.bytes, creation))
    };
    is_code(node)
        || batch
            .list(node)
            .is_some_and(|list| (0..list.child_count).any(|position| is_code(batch.child(list, position))))
}

fn reply_body(batch: &Batch, row: i32) -> i32 {
    let properties = properties::current();
    (0..2)
        .map(|position| batch.child_of(row, position))
        .find(|node| {
            batch.creation(*node).is_some_and(|creation| {
                type_is(batch.bytes, creation, b"view") && string_is(batch.bytes, creation, properties.direction, b"column")
            })
        })
        .unwrap_or(0)
}

fn reply_bar(batch: &Batch, body: i32) -> i32 {
    let bar = batch.child_of(body, 0);
    match batch.list(bar) {
        Some(list) if list.child_count == 1 => batch.child(list, 0),
        _ => bar,
    }
}

fn remember_reply(batch: &Batch, state: &mut Row, row: i32) {
    let properties = properties::current();
    let body = reply_body(batch, row);
    if body == 0 {
        return;
    }
    let bar = reply_bar(batch, body);
    let preview = batch.child_of(bar, 1);
    let (Some(bar_creation), Some(bar_list), Some(preview_creation)) = (batch.creation(bar), batch.list(bar), batch.creation(preview))
    else {
        return;
    };
    if !type_is(batch.bytes, preview_creation, b"stack") {
        return;
    }
    state.reply = preview;
    state.reply_bar = bar;
    state.bar_children = batch.children(bar_list, BUBBLE_CHILDREN);
    state.bar_padding = quad_property(batch.bytes, bar_creation, properties.padding).unwrap_or(NO_PADDING);
    state.bar_margin = quad_property(batch.bytes, bar_creation, properties.margin).unwrap_or(NO_PADDING);
}

fn find_block(state: &Row, id: i32) -> Option<Block> {
    state.blocks.iter().find(|block| block.id == id).cloned()
}

fn text_block(state: &Row) -> Option<Block> {
    state.blocks.iter().find(|block| !block.media).cloned()
}

fn create_quote_box(out: &mut Out, radius: f64) -> i32 {
    let properties = properties::current();
    let node = tag_id();
    out.create(node, b"view", 6);
    out.prop_string(properties.direction, b"column");
    out.prop_string(properties.align, b"start");
    out.prop_long(properties.background, QUOTE_COLOR);
    out.prop_number(properties.radius, (radius - 4.0).max(4.0));
    out.prop_quad(properties.padding, QUOTE_PADDING);
    out.prop_quad(properties.margin, [0.0, 0.0, 0.0, 6.0]);
    node
}

fn write_bar_layout(out: &mut Out, state: &Row, inside: bool) {
    let properties = properties::current();
    out.put_children(state.reply_bar, if inside { &[] } else { &state.bar_children });
    out.update(state.reply_bar, 2);
    out.prop_quad(properties.padding, if inside { NO_PADDING } else { state.bar_padding });
    out.prop_quad(properties.margin, if inside { NO_PADDING } else { state.bar_margin });
}

fn write_text_direction(out: &mut Out, text: i32, inside: bool) {
    let properties = properties::current();
    out.update(text, 1);
    if inside {
        out.prop_string(properties.direction, b"column");
    } else {
        out.prop_unset(properties.direction);
    }
}

fn is_ancestor(node: i32, of: i32) -> bool {
    let mut current = of;
    for _ in 0..64 {
        if current == 0 {
            return false;
        }
        if current == node {
            return true;
        }
        current = super::children::wire_discord_parent(current);
    }
    false
}

fn put_safe_children(out: &mut Out, parent: i32, children: &[i32]) {
    let safe: Vec<i32> = children.iter().copied().filter(|child| !is_ancestor(*child, parent)).collect();
    out.put_children(parent, &safe);
}

fn leading_children(state: &Row) -> Vec<i32> {
    let mut children = Vec::new();
    if state.name_shown && state.header != 0 {
        children.push(state.header);
    }
    if state.quote_box != 0 {
        children.push(state.quote_box);
    }
    children
}

fn write_host_layout(out: &mut Out, state: &Row, text: &Block) {
    let mut inserted = [0i32; INSERT_CAPACITY];
    let mut at_start: c_int = 0;
    let count = unsafe { wire_inserted_children(text.id, inserted.as_mut_ptr(), INSERT_CAPACITY as u32, &mut at_start) } as usize;
    let inserted = &inserted[..count.min(INSERT_CAPACITY)];
    let leading = leading_children(state);
    let mut children = leading.clone();
    if at_start != 0 {
        children.extend_from_slice(inserted);
    }
    children.extend_from_slice(&text.children);
    if at_start == 0 {
        children.extend_from_slice(inserted);
    }
    put_safe_children(out, text.id, &children);
    write_text_direction(out, text.id, !leading.is_empty());
}

fn write_column_layout(out: &mut Out, state: &Row) {
    if state.column == 0 || state.column_children.is_empty() {
        return;
    }
    let children: Vec<i32> = state
        .column_children
        .iter()
        .copied()
        .filter(|node| !(state.name_shown && *node == state.header))
        .collect();
    put_safe_children(out, state.column, &children);
}

fn write_quote_move(out: &mut Out, state: &mut Row, text: &Block, inside: bool, radius: f64) {
    if inside && state.quote_box == 0 {
        state.quote_box = create_quote_box(out, radius);
    }
    write_bar_layout(out, state, inside);
    if inside {
        put_safe_children(out, state.quote_box, &[state.reply]);
    }
    let quote_box = state.quote_box;
    if !inside {
        state.quote_box = 0;
    }
    write_host_layout(out, state, text);
    if inside || quote_box == 0 {
        return;
    }
    out.put_children(quote_box, &[]);
    out.remove(&[quote_box]);
}

fn write_time_parent_layout(out: &mut Out, state: &Row) {
    let children: Vec<i32> = state
        .time_parent_children
        .iter()
        .copied()
        .filter(|node| !(state.time_shown != TimePlacement::Header && *node == state.timestamp))
        .collect();
    put_safe_children(out, state.time_parent, &children);
}

fn can_move_time(state: &Row) -> bool {
    state.timestamp != 0 && state.time_parent != 0 && !state.time_parent_children.is_empty()
}

fn shows_inside(state: &Row) -> bool {
    state.reply_shown == ReplyPlacement::Inside || state.name_shown || state.time_shown != TimePlacement::Header
}

fn publish_time(state: &Row, listed: bool) {
    with_inline_times(|times| {
        if listed && state.time_shown == TimePlacement::Inline && !state.time_text.is_empty() && state.message != 0 {
            times.insert(state.message, (state.id, state.time_text.clone()));
        } else {
            times.retain(|_, (row, _)| *row != state.id);
        }
    });
}

fn unpublish_row(row: i32) {
    with_inline_times(|times| times.retain(|_, (owner, _)| *owner != row));
}

fn publish_times(backgrounds: &Backgrounds) -> Vec<u64> {
    let published: HashMap<u64, Vec<u8>> = with_inline_times(|times| {
        times.clear();
        for state in backgrounds.inside.iter().filter_map(|id| backgrounds.rows.get(id)) {
            if state.time_shown == TimePlacement::Inline && !state.time_text.is_empty() && state.message != 0 {
                times.insert(state.message, (state.id, state.time_text.clone()));
            }
        }
        times.iter().map(|(message, (_, text))| (*message, text.clone())).collect()
    });
    let mut guard = lock(&ENSURED_TIMES);
    let ensured = guard.get_or_insert_with(HashMap::new);
    let fresh = published
        .iter()
        .filter(|(message, text)| ensured.get(*message) != Some(*text))
        .map(|(message, _)| *message)
        .collect();
    *ensured = published;
    fresh
}

fn reapply_times(messages: &[u64], bytes: &[u8], out: &mut Out, changed: &mut c_int) {
    if messages.is_empty() {
        return;
    }
    let mut update = Out::default();
    if super::message_text::reapply_message_texts(&mut update, messages) && update.failed == 0 {
        put_original_once(out, bytes, changed);
        out.put(unsafe { core::slice::from_raw_parts(update.bytes, update.length) });
    }
    update.discard();
}

fn track_inside(inside: &mut Vec<i32>, state: &Row, shown: bool) {
    match inside.iter().position(|row| *row == state.id) {
        Some(position) if !shown => {
            inside.remove(position);
        }
        None if shown && inside.len() < INSIDE_CAPACITY => inside.push(state.id),
        _ => {}
    }
    publish_time(state, inside.contains(&state.id));
}

fn untrack_inside(inside: &mut Vec<i32>, row: i32) {
    inside.retain(|listed| *listed != row);
    unpublish_row(row);
}

pub fn inline_time(message: u64) -> Vec<u8> {
    with_inline_times(|times| times.get(&message).map(|(_, text)| text.clone()).unwrap_or_default())
}

fn write_avatar_shift(out: &mut Out, state: &mut Row, shifted: bool) {
    if state.avatar == 0 || shifted == state.avatar_shifted {
        return;
    }
    let properties = properties::current();
    let margin = state.avatar_margin;
    let top = margin[1] + if shifted { AVATAR_DECORATION_INSET } else { 0.0 };
    out.update(state.avatar, 1);
    out.prop_quad(properties.margin, [margin[0], top, margin[2], margin[3]]);
    state.avatar_shifted = shifted;
}

fn write_header_layout(out: &mut Out, state: &mut Row, inside: &mut Vec<i32>, text: Option<&Block>, name: bool, time: TimePlacement) {
    state.name_shown = name;
    state.time_shown = time;
    write_column_layout(out, state);
    write_avatar_shift(out, state, name);
    if let Some(text) = text {
        write_host_layout(out, state, text);
    }
    if can_move_time(state) {
        write_time_parent_layout(out, state);
    }
    track_inside(inside, state, shows_inside(state));
    if state.message != 0 {
        unsafe { wire_apply_message_text(out, state.message) };
    }
}

fn content_message(batch: &Batch, content: i32) -> u64 {
    let created = batch
        .creation(content)
        .and_then(|creation| read_identifier(batch.bytes, creation))
        .map(|(identifier, _)| message_of_identifier(identifier))
        .filter(|message| *message != 0);
    created
        .or_else(|| super::content_views::view_message(batch.bytes.as_ptr(), batch.spans, content))
        .unwrap_or(0)
}

fn host_row(backgrounds: &Backgrounds, parent: i32) -> Option<i32> {
    backgrounds
        .inside
        .iter()
        .copied()
        .find(|row| backgrounds.rows.get(row).is_some_and(|state| state.quoted_in == parent))
}

#[unsafe(export_name = "wireWriteHostLayout")]
pub unsafe extern "C" fn wire_write_host_layout(out: *mut Out, parent: i32) -> c_int {
    let Some(out) = (unsafe { out.as_mut() }) else { return 0 };
    with_backgrounds(|backgrounds| {
        let Some(state) = host_row(backgrounds, parent).and_then(|row| backgrounds.rows.get(&row)) else {
            return 0;
        };
        let Some(text) = find_block(state, parent) else { return 0 };
        write_host_layout(out, state, &text);
        1
    })
}

fn write_reply_style(out: &mut Out, state: &mut Row, inside: &mut Vec<i32>, mode: ReplyPlacement, color: u32, radius: f64) {
    if state.reply_shown == ReplyPlacement::Inside && mode != ReplyPlacement::Inside {
        if let Some(quoted) = find_block(state, state.quoted_in) {
            write_quote_move(out, state, &quoted, false, radius);
        }
        if !state.name_shown && state.time_shown != TimePlacement::Inline {
            state.quoted_in = 0;
        }
    }
    if mode == ReplyPlacement::Inside
        && let Some(text) = text_block(state)
    {
        write_quote_move(out, state, &text, true, radius);
        state.quoted_in = text.id;
    }
    write_preview_style(out, state.reply, mode, color);
    state.reply_shown = mode;
    track_inside(inside, state, shows_inside(state));
}

fn read_time_text(bytes: &[u8], span: &Span) -> Vec<u8> {
    find_string(bytes, span, properties::current().text)
        .filter(|text| text.len() < TIME_TEXT_CAPACITY)
        .map(<[u8]>::to_vec)
        .unwrap_or_default()
}

fn known_parent_of(links: &HashMap<i32, i32>, id: i32) -> i32 {
    match parent_in(links, id) {
        0 if id != 0 => super::children::wire_discord_parent(id),
        parent => parent,
    }
}

fn remember_timestamp(batch: &Batch, links: &HashMap<i32, i32>, state: &mut Row) {
    let timestamps = batch.spans.iter().filter(|span| span.op == Op::Create || span.op == Op::Update);
    for span in timestamps {
        if tracked_value(span.id, TRACK_TIMESTAMP).is_none() {
            continue;
        }
        let parent = known_parent_of(links, span.id);
        let mut ancestor = parent;
        for _ in 0..6 {
            if ancestor == 0 || ancestor == state.header {
                break;
            }
            ancestor = known_parent_of(links, ancestor);
        }
        if parent == 0 || ancestor != state.header {
            continue;
        }
        let children = match batch.list(parent) {
            Some(list) => batch.children(list, BUBBLE_CHILDREN * 2),
            None => super::children::discord_children(parent)
                .into_iter()
                .take(BUBBLE_CHILDREN * 2)
                .collect(),
        };
        let text = read_time_text(batch.bytes, span);
        if children.is_empty() || (span.op == Op::Update && text.is_empty()) {
            continue;
        }
        state.timestamp = span.id;
        state.time_parent = parent;
        state.time_text = text;
        state.time_parent_children = children;
        return;
    }
}

fn remember_avatar(batch: &Batch, links: &HashMap<i32, i32>, rows: &HashSet<i32>, state: &mut Row) {
    let margin = properties::current().margin;
    let found = batch
        .spans
        .iter()
        .filter(|span| span.op == Op::Create)
        .find(|span| tracked_value(span.id, TRACK_AVATAR).is_some() && row_for_node(links, rows, span.id) == state.id);
    if let Some(span) = found {
        state.avatar = span.id;
        state.avatar_margin = quad_property(batch.bytes, span, margin).unwrap_or(NO_PADDING);
    }
}

fn holds_author(node: i32, depth: u32) -> bool {
    if tracked_value(node, TRACK_AUTHOR).is_some() {
        return true;
    }
    depth < 4 && discord_children(node).into_iter().any(|child| holds_author(child, depth + 1))
}

fn header_is_current(state: &Row) -> bool {
    state.header != 0 && state.column != 0 && discord_children(state.column).contains(&state.header)
}

fn remember_known_header(batch: &Batch, links: &HashMap<i32, i32>, state: &mut Row) {
    if state.column == 0 {
        return;
    }
    let column_children: Vec<i32> = discord_children(state.column).into_iter().take(BUBBLE_CHILDREN * 2).collect();
    let text = text_block(state).map_or(0, |block| block.id);
    let Some(header) = column_children
        .iter()
        .copied()
        .find(|child| holds_author(*child, 0) && !is_ancestor(*child, text))
    else {
        return;
    };
    state.header = header;
    state.column_children = column_children;
    remember_timestamp(batch, links, state);
}

fn remember_header(batch: &Batch, links: &HashMap<i32, i32>, authors: &[i32], state: &mut Row) {
    if state.column == 0 {
        return;
    }
    let column_children = match batch.list(state.column) {
        Some(list) => batch.children(list, BUBBLE_CHILDREN * 2),
        None => discord_children(state.column).into_iter().take(BUBBLE_CHILDREN * 2).collect(),
    };
    if column_children.is_empty() {
        return;
    }
    for author in authors {
        let mut node = *author;
        let mut parent = known_parent(links, node);
        for _ in 0..8 {
            if parent == 0 || parent == state.column {
                break;
            }
            node = parent;
            parent = known_parent(links, node);
        }
        if parent != state.column {
            continue;
        }
        state.header = node;
        state.column_children = column_children;
        remember_timestamp(batch, links, state);
        return;
    }
    remember_known_header(batch, links, state);
}

fn keep_quotes_inside(batch: &Batch, backgrounds: &mut Backgrounds, out: &mut Out, changed: &mut c_int) {
    let Backgrounds { rows, inside } = backgrounds;
    for row in inside.clone() {
        let Some(state) = rows.get_mut(&row) else {
            untrack_inside(inside, row);
            continue;
        };
        let quoting = state.reply_shown == ReplyPlacement::Inside;
        if !quoting && !state.name_shown && state.time_shown == TimePlacement::Header {
            untrack_inside(inside, row);
            continue;
        }
        let bar_list = if quoting { batch.list(state.reply_bar) } else { None };
        let text_list = batch.list(state.quoted_in);
        let column_list = if state.name_shown { batch.list(state.column) } else { None };
        let time_list = if state.time_shown != TimePlacement::Header {
            batch.list(state.time_parent)
        } else {
            None
        };
        if let Some(time_list) = time_list {
            state.time_parent_children = batch.children(time_list, BUBBLE_CHILDREN * 2);
            put_original_once(out, batch.bytes, changed);
            write_time_parent_layout(out, state);
        }
        if bar_list.is_none() && text_list.is_none() && column_list.is_none() {
            continue;
        }
        if let Some(bar_list) = bar_list {
            state.bar_children = batch.children(bar_list, BUBBLE_CHILDREN);
            if state.bar_children.len() > 1 {
                state.reply = state.bar_children[1];
            }
        }
        if let Some(column_list) = column_list {
            state.column_children = batch.children(column_list, BUBBLE_CHILDREN * 2);
        }
        let Some(index) = state.blocks.iter().position(|block| block.id == state.quoted_in) else {
            continue;
        };
        if text_list.is_some() {
            remember_block_children(batch, &mut state.blocks[index]);
        }
        put_original_once(out, batch.bytes, changed);
        let text = state.blocks[index].clone();
        if quoting {
            write_quote_move(out, state, &text, true, state.applied_radius);
            if bar_list.is_some() {
                write_preview_style(out, state.reply, ReplyPlacement::Inside, state.applied_color);
            }
        }
        if column_list.is_some() {
            write_column_layout(out, state);
        }
        if !quoting {
            write_host_layout(out, state, &text);
        }
    }
}

fn row_for_column(rows: &HashMap<i32, Row>, column: i32) -> Option<i32> {
    if column == 0 {
        return None;
    }
    rows.values().find(|state| state.column == column).map(|state| state.id)
}

fn reset_bubble_nodes(state: &mut Row, inside: &mut Vec<i32>) {
    state.blocks.clear();
    state.shown_blocks = 0;
    state.column = 0;
    state.reply = 0;
    state.bubble = false;
    state.reply_shown = ReplyPlacement::Above;
    state.reply_bar = 0;
    state.quoted_in = 0;
    state.quote_box = 0;
    state.header = 0;
    state.timestamp = 0;
    state.time_parent = 0;
    state.time_parent_children.clear();
    state.column_children.clear();
    state.name_shown = false;
    state.time_shown = TimePlacement::Header;
    state.avatar = 0;
    state.avatar_shifted = false;
    state.corners = -1;
    track_inside(inside, state, false);
}

fn remember_block(batch: &Batch, links: &HashMap<i32, i32>, state: &mut Row, id: i32, media: bool) {
    let Some(index) = add_block(state, id, media) else { return };
    let padding = properties::current().padding;
    if media
        && let Some(quad) = batch
            .creation(id)
            .and_then(|creation| quad_property(batch.bytes, creation, padding))
    {
        state.blocks[index].padding = quad;
    }
    remember_block_children(batch, &mut state.blocks[index]);
    let column = parent_in(links, id);
    if column != 0 {
        state.column = column;
    }
}

fn replace_text_block(state: &mut Row, id: i32) {
    let Some(index) = state.blocks.iter().position(|block| !block.media && block.id != id) else {
        return;
    };
    if state.quoted_in == state.blocks[index].id {
        state.quoted_in = id;
    }
    state.blocks[index] = Block { id, ..Block::default() };
    if state.shown_blocks > index {
        state.shown_blocks = index;
    }
}

fn bubble_late_blocks(
    batch: &Batch,
    links: &HashMap<i32, i32>,
    backgrounds: &mut Backgrounds,
    blocks: &[i32],
    media: bool,
    out: &mut Out,
    changed: &mut c_int,
) {
    for block in blocks.iter().copied().filter(|block| *block != 0) {
        let Some(row) = ancestor_row(&backgrounds.rows, links, block) else {
            continue;
        };
        let Backgrounds { rows, inside } = backgrounds;
        let Some(state) = rows.get_mut(&row) else { continue };
        if !media {
            replace_text_block(state, block);
        }
        remember_block(batch, links, state, block, media);
        let header = state.header;
        if !header_is_current(state) {
            state.header = 0;
            remember_known_header(batch, links, state);
        }
        let moved = state.header != header;
        if moved {
            state.name_shown = false;
        }
        if !state.bubble || moved {
            if state.has_original {
                apply_background_style(out, state, inside, Some(batch.bytes), changed);
            }
            continue;
        }
        if state.shown_blocks == state.blocks.len() {
            continue;
        }
        put_original_once(out, batch.bytes, changed);
        for index in state.shown_blocks..state.blocks.len() {
            write_block_update(
                out,
                &state.blocks[index],
                true,
                state.applied_color,
                state.applied_radius,
                state.applied_padding,
            );
        }
        state.shown_blocks = state.blocks.len();
        if state.reply_shown == ReplyPlacement::Inside
            && let Some(quoted) = find_block(state, state.quoted_in)
        {
            write_quote_move(out, state, &quoted, true, state.applied_radius);
        }
    }
}

fn has_block_content(state: &Row) -> bool {
    state.blocks.iter().any(|block| !block.children.is_empty() || block.card != 0)
}

fn background_style(state: &Row) -> Option<(c_int, AstralMessageBackgroundStyle)> {
    let mut style = AstralMessageBackgroundStyle::default();
    let Some(decorator) = BACKGROUND_DECORATOR.get::<AstralMessageBackgroundDecorator>() else {
        return Some((0, style));
    };
    let context = AstralMessageBackgroundContext {
        message_id: state.message,
        user_id: state.user,
        channel_id: state.channel,
    };
    let result = unsafe { decorator(&context, &mut style) };
    if result < 0 {
        wire_mark_refresh_busy();
        return None;
    }
    Some((result, style))
}

fn apply_background_style(out: &mut Out, state: &mut Row, inside: &mut Vec<i32>, original: Option<&[u8]>, changed: &mut c_int) {
    let Some((result, style)) = background_style(state) else { return };
    let active = result > 0 && valid_style(&style);
    let bubble = active && style.bubble != 0 && has_block_content(state);
    let text = text_block(state);
    let mut reply = if bubble && state.reply != 0 {
        style.reply
    } else {
        ReplyPlacement::Above
    };
    if reply == ReplyPlacement::Inside && text.is_none() {
        reply = ReplyPlacement::Bubble;
    }
    let name = bubble && style.name_inside != 0 && state.header != 0 && !state.column_children.is_empty() && text.is_some();
    let mut time = if bubble && can_move_time(state) {
        style.time
    } else {
        TimePlacement::Header
    };
    if time == TimePlacement::Inline && !name {
        time = TimePlacement::Header;
    }
    let color = if active { style.color } else { state.original_color };
    let radius = if active { style.radius } else { state.original_radius };
    if active == state.active
        && bubble == state.bubble
        && reply == state.reply_shown
        && name == state.name_shown
        && time == state.time_shown
        && style.grouping == state.grouping
        && (!bubble || state.shown_blocks == state.blocks.len())
        && (!active || (color == state.applied_color && radius == state.applied_radius && bubble_padding(&style) == state.applied_padding))
    {
        return;
    }
    state.applied_padding = if active { bubble_padding(&style) } else { BUBBLE_PADDING };
    if style.grouping != state.grouping || (bubble && !state.bubble) {
        state.grouping = style.grouping;
        state.corners = grouped_corners(state);
    }
    match original {
        Some(original) => put_original_once(out, original, changed),
        None => *changed = 1,
    }
    if bubble || state.bubble {
        write_bubble_update(out, state, bubble, color, radius);
    }
    if reply != state.reply_shown {
        write_reply_style(out, state, inside, reply, color, radius);
    }
    if name != state.name_shown || time != state.time_shown {
        let text = if name {
            text_block(state)
        } else {
            find_block(state, state.quoted_in)
        };
        write_header_layout(out, state, inside, text.as_ref(), name, time);
        match text {
            Some(text) if name => state.quoted_in = text.id,
            _ if state.reply_shown != ReplyPlacement::Inside => state.quoted_in = 0,
            _ => {}
        }
        track_inside(inside, state, shows_inside(state));
    }
    if bubble {
        write_background_update(out, state.id, state.original_color, state.original_radius);
    } else {
        write_background_update(out, state.id, color, radius);
    }
    state.bubble = bubble;
    write_group_gap(out, state);
    state.shown_blocks = if bubble { state.blocks.len() } else { 0 };
    if active {
        state.applied_color = color;
        state.applied_radius = radius;
    }
    state.active = active;
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_wire_refresh_message_backgrounds(buffer: *mut AstralWireBuffer) -> c_int {
    let Some(buffer) = (unsafe { buffer.as_mut() }) else { return 0 };
    *buffer = AstralWireBuffer {
        bytes: core::ptr::null_mut(),
        length: 0,
    };
    let mut out = Out::batch();
    let channel = current_channel();
    let mut any: c_int = 0;
    with_backgrounds(|backgrounds| {
        let Backgrounds { rows, inside } = &mut *backgrounds;
        for state in rows.values_mut() {
            if state.id <= 0 || !state.has_original || (channel != 0 && state.channel != 0 && state.channel != channel) {
                continue;
            }
            apply_background_style(&mut out, state, inside, None, &mut any);
        }
        let fresh = publish_times(backgrounds);
        let mut update = Out::default();
        if super::message_text::reapply_message_texts(&mut update, &fresh) && update.failed == 0 {
            out.put(unsafe { core::slice::from_raw_parts(update.bytes, update.length) });
            any = 1;
        }
        update.discard();
    });
    if any == 0 {
        out.discard();
        return 0;
    }
    c_int::from(out.finish(buffer))
}

fn span_sets(bytes: &[u8], span: &Span, wanted: u16) -> bool {
    let (mut cursor, count) = property_cursor(bytes, span);
    for _ in 0..count {
        if cursor.failed() {
            return false;
        }
        let property = cursor.read16();
        let tag = cursor.read8();
        if property == wanted {
            return true;
        }
        cursor.skip_value(tag, 0);
    }
    false
}

fn release_reused_blocks(batch: &Batch, backgrounds: &mut Backgrounds, out: &mut Out, changed: &mut c_int) {
    let properties = properties::current();
    for span in batch.spans.iter().filter(|span| span.op == Op::Update) {
        if !span_sets(batch.bytes, span, properties.accessibility_id) {
            continue;
        }
        if find_string(batch.bytes, span, properties.accessibility_id).is_some_and(|identifier| identifier.starts_with(CONTENT_PREFIX)) {
            continue;
        }
        for state in backgrounds.rows.values_mut() {
            let Some(index) = state.blocks.iter().position(|block| block.id == span.id) else {
                continue;
            };
            state.blocks.remove(index);
            state.shown_blocks = state.shown_blocks.min(state.blocks.len());
            let reset: Vec<u16> = [properties.background, properties.radius, properties.padding]
                .into_iter()
                .filter(|property| !span_sets(batch.bytes, span, *property))
                .collect();
            if reset.is_empty() {
                continue;
            }
            put_original_once(out, batch.bytes, changed);
            out.update(span.id, reset.len() as u16);
            for property in reset {
                out.prop_unset(property);
            }
        }
    }
}

fn forget_removed_rows(batch: &Batch, backgrounds: &mut Backgrounds) {
    for span in batch.spans.iter().filter(|span| span.op == Op::Remove) {
        for position in 0..span.child_count {
            let id = batch.child(span, position);
            if backgrounds.rows.remove(&id).is_some() {
                untrack_inside(&mut backgrounds.inside, id);
            }
        }
    }
}

fn listed_header(rows: &HashMap<i32, Row>, id: i32) -> Option<i32> {
    rows.get(&id).filter(|state| state.has_original).map(|state| state.header)
}

fn remember_inner_views(batch: &Batch, rows: &mut HashMap<i32, Row>) {
    let padding = properties::current().padding;
    for span in batch.spans.iter().filter(|span| span.op == Op::SetChildren && span.child_count > 0) {
        let Some(state) = rows.get_mut(&span.id).filter(|state| state.has_original) else {
            continue;
        };
        let inner = batch.child(span, 0);
        if inner == state.inner {
            continue;
        }
        let Some(creation) = batch.creation(inner) else { continue };
        state.inner = inner;
        state.inner_padding = quad_property(batch.bytes, creation, padding).unwrap_or(NO_PADDING);
        state.group_gap = None;
    }
}

fn group_rows(batch: &Batch, rows: &mut HashMap<i32, Row>, out: &mut Out, changed: &mut c_int) {
    remember_inner_views(batch, rows);
    for span in batch
        .spans
        .iter()
        .filter(|span| span.op == Op::SetChildren && span.child_count >= 2)
    {
        let mut previous = false;
        for position in 0..span.child_count {
            let id = batch.child(span, position);
            let Some(header) = listed_header(rows, id) else {
                previous = false;
                continue;
            };
            let next = if position + 1 < span.child_count {
                listed_header(rows, batch.child(span, position + 1))
            } else {
                None
            };
            let start = !previous || header != 0;
            let end = next.is_none_or(|header| header != 0);
            previous = true;
            let Some(state) = rows.get_mut(&id) else { continue };
            state.group_start = start;
            state.group_end = end;
            let corners = grouped_corners(state);
            if !state.bubble {
                state.corners = corners;
                continue;
            }
            if corners == state.corners && wanted_group_gap(state) == state.group_gap {
                continue;
            }
            put_original_once(out, batch.bytes, changed);
            if corners != state.corners {
                state.corners = corners;
                write_corners(out, state, true, state.applied_radius);
            }
            write_group_gap(out, state);
        }
    }
}

fn row_state(rows: &mut HashMap<i32, Row>, id: i32) -> Option<&mut Row> {
    if id == 0 {
        return None;
    }
    if !rows.contains_key(&id) && rows.len() >= MAX_DECORATED {
        let evicted = rows.iter().find(|(_, state)| !state.has_original).map(|(key, _)| *key)?;
        rows.remove(&evicted);
    }
    Some(rows.entry(id).or_insert_with(|| Row {
        id,
        corners: -1,
        group_end: true,
        group_start: true,
        ..Row::default()
    }))
}

fn finish_backgrounds(
    batch: &Batch,
    backgrounds: &mut Backgrounds,
    links: &HashMap<i32, i32>,
    contents: &[i32],
    media: &[i32],
    out: &mut Out,
    changed: &mut c_int,
) {
    release_reused_blocks(batch, backgrounds, out, changed);
    bubble_late_blocks(batch, links, backgrounds, contents, false, out, changed);
    bubble_late_blocks(batch, links, backgrounds, media, true, out, changed);
    keep_quotes_inside(batch, backgrounds, out, changed);
    group_rows(batch, &mut backgrounds.rows, out, changed);
    forget_removed_rows(batch, backgrounds);
    let mut retimed = refresh_time_texts(batch, backgrounds);
    for message in publish_times(backgrounds) {
        if !retimed.contains(&message) {
            retimed.push(message);
        }
    }
    reapply_times(&retimed, batch.bytes, out, changed);
}

fn refresh_time_texts(batch: &Batch, backgrounds: &mut Backgrounds) -> Vec<u64> {
    let mut retimed = Vec::new();
    for span in batch.spans.iter().filter(|span| span.op == Op::Update) {
        let text = read_time_text(batch.bytes, span);
        if text.is_empty() {
            continue;
        }
        let Some(state) = backgrounds.rows.values_mut().find(|state| state.timestamp == span.id) else {
            continue;
        };
        if text == state.time_text {
            continue;
        }
        state.time_text = text;
        if state.time_shown == TimePlacement::Inline && state.message != 0 {
            retimed.push(state.message);
        }
    }
    retimed
}

fn message_backgrounds(batch: &Batch, out: &mut Out, changed: &mut c_int) {
    let properties = properties::current();
    let mut links: HashMap<i32, i32> = HashMap::new();
    let mut row_ids: HashSet<i32> = HashSet::new();
    let mut listed: Vec<ListedRow> = Vec::new();
    let mut contents: Vec<i32> = Vec::new();
    let mut media: Vec<i32> = Vec::new();
    let mut authors: Vec<i32> = Vec::new();
    for span in batch.spans {
        if span.op == Op::SetChildren {
            for position in 0..span.child_count {
                let node = batch.child(span, position);
                if node != 0 {
                    links.insert(node, span.id);
                }
            }
        } else if (span.op == Op::Create || span.op == Op::Update)
            && contents.len() < ROW_CAPACITY
            && find_string(batch.bytes, span, properties.accessibility_id).is_some_and(|text| text.starts_with(CONTENT_PREFIX))
        {
            contents.push(span.id);
        } else if span.op == Op::Create
            && listed.len() < ROW_CAPACITY
            && string_is(batch.bytes, span, properties.accessibility_id, b"message-row")
        {
            listed.push(ListedRow {
                id: span.id,
                ..ListedRow::default()
            });
            row_ids.insert(span.id);
        } else if media.len() < MEDIA_CAPACITY && is_media_block(batch, span) {
            media.push(span.id);
        }
    }
    with_backgrounds(|backgrounds| {
        for span in batch.spans {
            if listed.len() >= ROW_CAPACITY {
                break;
            }
            if !matches!(span.op, 1..=3) {
                continue;
            }
            let Some(state) = backgrounds.rows.get(&span.id).filter(|state| state.has_original) else {
                continue;
            };
            if batch.was_removed(span.id) || listed.iter().any(|row| row.id == span.id) {
                continue;
            }
            listed.push(ListedRow {
                id: state.id,
                message: state.message,
                user: state.user,
                content: 0,
            });
            row_ids.insert(state.id);
        }
        if listed.is_empty() {
            finish_backgrounds(batch, backgrounds, &links, &contents, &media, out, changed);
            return;
        }
        for span in batch.spans.iter().filter(|span| span.op == Op::Create || span.op == Op::Update) {
            let Some(message) = tracked_value(span.id, TRACK_AUTHOR) else {
                continue;
            };
            if authors.len() < ROW_CAPACITY {
                authors.push(span.id);
            }
            let id = row_for_node(&links, &row_ids, span.id);
            let Some(row) = listed.iter_mut().find(|row| row.id == id) else {
                continue;
            };
            if row.message != 0 && batch.creation(id).is_none() {
                continue;
            }
            row.message = message;
            row.user = author_of(message);
        }
        for content in contents.iter_mut() {
            let id = row_for_node(&links, &row_ids, *content);
            let Some(row) = listed.iter_mut().find(|row| row.id == id) else {
                continue;
            };
            row.content = *content;
            let shown = content_message(batch, *content);
            if shown != 0 {
                row.message = shown;
            }
            *content = 0;
        }
        let decorating = BACKGROUND_DECORATOR.get::<AstralMessageBackgroundDecorator>().is_some();
        for row in &listed {
            let creation = batch.creation(row.id);
            let Backgrounds { rows, inside } = &mut *backgrounds;
            if row.message == 0 && !rows.contains_key(&row.id) && !decorating {
                continue;
            }
            let Some(state) = row_state(rows, row.id) else { continue };
            if let Some(creation) = creation {
                capture_original(batch, Some(creation), state);
                reset_bubble_nodes(state, inside);
                remember_reply(batch, state, row.id);
                state.active = false;
                state.message = row.message;
                state.user = row.user;
                state.channel = current_channel();
            } else if row.message != 0 {
                if state.message != row.message {
                    state.time_text.clear();
                    remember_timestamp(batch, &links, state);
                }
                state.message = row.message;
                state.user = row.user;
                state.channel = current_channel();
            }
            if !state.has_original {
                capture_original(batch, creation, state);
            }
            if row.content != 0 {
                replace_text_block(state, row.content);
                remember_block(batch, &links, state, row.content, false);
            }
            for block in media.iter_mut() {
                if *block == 0 || row_for_node(&links, &row_ids, *block) != row.id {
                    continue;
                }
                remember_block(batch, &links, state, *block, true);
                *block = 0;
            }
            if !header_is_current(state) {
                let header = state.header;
                state.header = 0;
                remember_header(batch, &links, &authors, state);
                if state.header != header && state.name_shown {
                    state.name_shown = false;
                }
            }
            if state.avatar == 0 {
                remember_avatar(batch, &links, &row_ids, state);
            }
            apply_background_style(out, state, inside, Some(batch.bytes), changed);
        }
        finish_backgrounds(batch, backgrounds, &links, &contents, &media, out, changed);
    });
}

fn encode_base64(bytes: &[u8], into: &mut Vec<u8>) {
    for chunk in bytes.chunks(3) {
        let value = (u32::from(chunk[0]) << 16) | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8) | u32::from(*chunk.get(2).unwrap_or(&0));
        into.push(BASE64[(value >> 18) as usize & 63]);
        into.push(BASE64[(value >> 12) as usize & 63]);
        into.push(if chunk.len() > 1 {
            BASE64[(value >> 6) as usize & 63]
        } else {
            b'='
        });
        into.push(if chunk.len() > 2 { BASE64[value as usize & 63] } else { b'=' });
    }
}

fn encoded_icon(path: &[u8]) -> Option<Vec<u8>> {
    let data = std::fs::read(std::ffi::OsStr::from_bytes(path)).ok()?;
    if data.is_empty() || data.len() > MAX_ICON_FILE_BYTES {
        return None;
    }
    let webp = path.windows(5).any(|window| window == b".webp");
    let mut uri = if webp {
        b"data:image/webp;base64,".to_vec()
    } else {
        b"data:image/png;base64,".to_vec()
    };
    encode_base64(&data, &mut uri);
    Some(uri)
}

fn icon_uri(icon: &[u8]) -> Option<Vec<u8>> {
    let Some(path) = icon.strip_prefix(b"file://") else {
        return Some(icon.to_vec());
    };
    if let Some((_, uri)) = lock(&ICONS).0.iter().find(|(cached, _)| cached == path) {
        return Some(uri.clone());
    }
    let uri = encoded_icon(path)?;
    let mut icons = lock(&ICONS);
    let slot = icons.1 % ICON_CACHE_SIZE;
    icons.1 = icons.1.wrapping_add(1);
    let entry = (path.to_vec(), uri.clone());
    if slot < icons.0.len() {
        icons.0[slot] = entry;
    } else {
        icons.0.push(entry);
    }
    Some(uri)
}

fn build_icon_tag(out: &mut Out, tag: &AstralTag, nodes: &mut Vec<i32>) -> i32 {
    let Some(uri) = icon_uri(field(&tag.icon)) else { return 0 };
    let properties = properties::current();
    let icon = tag_id();
    let action = field(&tag.action_id);
    let count = 6 + u16::from(tag.tint != 0) + if action.is_empty() { 0 } else { 2 };
    out.create(icon, b"image", count);
    out.prop_string(properties.uri, &uri);
    out.prop_number(properties.width, ICON_TAG_SIZE);
    out.prop_number(properties.height, ICON_TAG_SIZE);
    out.prop_string(properties.scale, b"fit");
    out.prop_number(properties.shrink, 0.0);
    if action.is_empty() {
        out.prop_bool(properties.accessibility_hidden, true);
    } else {
        out.prop_string(properties.accessibility_label, field(&tag.text));
        out.prop_string(properties.accessibility_id, action);
        out.prop_events(properties.on, b"press");
    }
    if tag.tint != 0 {
        out.prop_long(properties.tint, i64::from(tag.tint));
    }
    nodes.push(icon);
    icon
}

fn build_tag(out: &mut Out, tag: &AstralTag, nodes: &mut Vec<i32>) -> i32 {
    if tag.shape == TAG_ICON {
        return build_icon_tag(out, tag, nodes);
    }
    let properties = properties::current();
    let original_poster = tag.shape == TAG_ORIGINAL_POSTER;
    let plain = tag.shape == TAG_TEXT;
    let action = field(&tag.action_id);
    let text = field(&tag.text);
    let pill = tag_id();
    out.create(pill, b"view", if action.is_empty() { 6 } else { 9 });
    out.prop_string(properties.align, b"center");
    if tag.gradient_count >= 2 && !plain {
        let count = tag.gradient_count.min(3) as usize;
        out.prop_gradient(properties.background_gradient, 90.0, &tag.gradient[..count]);
    } else {
        out.prop_long(properties.background, if plain { 0 } else { i64::from(tag.background) });
    }
    out.prop_string(properties.direction, b"row");
    out.prop_number(properties.height, if original_poster { 16.0 } else { 18.0 });
    let inset = if plain {
        0.0
    } else if original_poster {
        5.0
    } else {
        4.0
    };
    out.prop_quad(properties.padding, [inset, 0.0, inset, 0.0]);
    out.prop_number(properties.radius, if original_poster { 8.0 } else { 4.0 });
    if !action.is_empty() {
        out.prop_string(properties.accessibility_id, action);
        out.prop_string(properties.accessibility_label, text);
        out.prop_events(properties.on, b"press");
    }
    let label = tag_id();
    out.create(label, b"text", if original_poster { 8 } else { 7 });
    out.prop_long(properties.color, i64::from(tag.foreground));
    out.prop_number(properties.font_size, if original_poster { 10.0 } else { 12.0 });
    out.prop_int(
        properties.font_weight,
        if original_poster {
            700
        } else if plain {
            500
        } else {
            600
        },
    );
    out.prop_number(properties.line_height, if original_poster { 13.0 } else { 16.0 });
    out.prop_int(properties.max_lines, 1);
    out.prop_string(properties.text, text);
    out.prop_bool(properties.noto_fallback, true);
    if original_poster {
        out.prop_string(properties.text_transform, b"uppercase");
    }
    out.put_children(pill, &[label]);
    nodes.push(pill);
    nodes.push(label);
    pill
}

#[unsafe(export_name = "wireBuildTag")]
pub unsafe extern "C" fn wire_build_tag(out: *mut Out, tag: *const AstralTag, slot: *mut Decorated) -> i32 {
    let (Some(out), Some(tag), Some(slot)) = (unsafe { out.as_mut() }, unsafe { tag.as_ref() }, unsafe { slot.as_mut() }) else {
        return 0;
    };
    let mut nodes = Vec::new();
    let root = build_tag(out, tag, &mut nodes);
    for node in nodes {
        let index = slot.count as usize;
        if index < slot.tags.len() {
            slot.tags[index] = node;
            slot.count += 1;
        }
    }
    root
}

fn tag_signature(tags: &[AstralTag]) -> u64 {
    let bytes = unsafe { core::slice::from_raw_parts(tags.as_ptr().cast::<u8>(), core::mem::size_of_val(tags)) };
    let hash = bytes.iter().fold(1_469_598_103_934_665_603u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(1_099_511_628_211)
    });
    hash ^ tags.len() as u64
}

fn decorate_author(decorator: AstralDecorator, message: u64) -> Option<([AstralTag; MAX_TAGS], usize)> {
    let fallbacks = crate::lua::host::hooks::busy_fallbacks();
    let context = AstralDecorationContext {
        prefix: c"message.author:".as_ptr(),
        message_id: message,
        user_id: author_of(message),
        channel_id: current_channel(),
    };
    let mut tags = [AstralTag::zeroed(); MAX_TAGS];
    let count = unsafe { decorator(&context, tags.as_mut_ptr(), MAX_TAGS) };
    STAT_AUTHORS.fetch_add(1, Ordering::Relaxed);
    if context.user_id != 0 {
        STAT_WITH_USER.fetch_add(1, Ordering::Relaxed);
    }
    if context.channel_id != 0 {
        STAT_WITH_CHANNEL.fetch_add(1, Ordering::Relaxed);
    }
    if count != 0 {
        STAT_TAGGED.fetch_add(1, Ordering::Relaxed);
    }
    if crate::lua::host::hooks::busy_fallbacks() != fallbacks {
        return None;
    }
    Some((tags, count.min(MAX_TAGS)))
}

fn put_tagged_children(out: &mut Out, parent: i32, children: &[i32], slot: &Tags) {
    let timestamp = with_tracked(|tracked| children.iter().position(|node| kind_of(tracked, *node) == TRACK_TIMESTAMP));
    let insert_at = timestamp.unwrap_or(children.len());
    let trailing_at = timestamp.map_or(children.len(), |position| position + 1);
    let mut merged = Vec::with_capacity(children.len() + slot.roots.len() + slot.trailing.len());
    for position in 0..=children.len() {
        if merged.len() >= MERGED_CHILDREN {
            break;
        }
        if position == insert_at {
            merged.extend_from_slice(&slot.roots);
        }
        if position == trailing_at {
            merged.extend_from_slice(&slot.trailing);
        }
        if let Some(node) = children.get(position) {
            merged.push(*node);
        }
    }
    out.put_children(parent, &merged);
}

fn write_tags(out: &mut Out, parent: i32, children: &[i32], tags: &[AstralTag], slot: &mut Tags) {
    remove_tags(out, slot);
    slot.signature = tag_signature(tags);
    for tag in tags {
        let root = build_tag(out, tag, &mut slot.nodes);
        if root == 0 {
            continue;
        }
        if tag.shape == TAG_TEXT {
            slot.trailing.push(root);
        } else {
            slot.roots.push(root);
        }
    }
    put_tagged_children(out, parent, children, slot);
}

fn decorated_slot(decorated: &mut HashMap<i32, Tags>, parent: i32) -> Option<&mut Tags> {
    if !decorated.contains_key(&parent) && decorated.len() >= MAX_DECORATED {
        return None;
    }
    Some(decorated.entry(parent).or_default())
}

fn author_in(children: &[i32]) -> Option<u64> {
    with_tracked(|tracked| {
        children
            .iter()
            .find_map(|node| tracked.get(node).filter(|slot| slot.kind == TRACK_AUTHOR).map(|slot| slot.value))
    })
}

fn keep_tags(batch: &Batch, out: &mut Out, changed: &mut c_int, parent: i32, children: &[i32], message: u64) {
    with_decorated(|decorated| {
        let Some(slot) = decorated.get_mut(&parent) else { return };
        if slot.message != message || (slot.roots.is_empty() && slot.trailing.is_empty()) {
            return;
        }
        put_original_once(out, batch.bytes, changed);
        put_tagged_children(out, parent, children, slot);
    });
}

fn decorate_tags(batch: &Batch, decorator: AstralDecorator, out: &mut Out, changed: &mut c_int) {
    let mut handled = Vec::new();
    for span in batch.spans {
        if span.op != Op::SetChildren || span.child_count == 0 || span.child_count as usize > MAX_TAGGED_CHILDREN {
            continue;
        }
        let children = batch.children(span, MAX_TAGGED_CHILDREN);
        let Some(message) = author_in(&children) else { continue };
        handled.push(span.id);
        apply_author_tags(batch, decorator, out, changed, span.id, &children, message);
    }
    for (parent, message) in recycled_authors(batch, &handled) {
        let children = discord_children(parent);
        if children.is_empty() || children.len() > MAX_TAGGED_CHILDREN {
            continue;
        }
        apply_author_tags(batch, decorator, out, changed, parent, &children, message);
    }
}

fn recycled_authors(batch: &Batch, handled: &[i32]) -> Vec<(i32, u64)> {
    let accessibility = properties::current().accessibility_id;
    let mut recycled = Vec::new();
    for span in batch.spans.iter().filter(|span| span.op == Op::Update) {
        let Some(message) = find_string(batch.bytes, span, accessibility).and_then(|text| prefixed(text, b"message.author:")) else {
            continue;
        };
        let parent = super::children::wire_discord_parent(span.id);
        if parent == 0 || handled.contains(&parent) {
            continue;
        }
        let stale = with_decorated(|decorated| decorated.get(&parent).is_some_and(|slot| slot.message != message));
        if stale {
            recycled.push((parent, message));
        }
    }
    recycled
}

fn without_moved_times(parent: i32, children: &[i32]) -> Vec<i32> {
    let moved: Vec<i32> = with_backgrounds(|backgrounds| {
        backgrounds
            .rows
            .values()
            .filter(|state| state.time_parent == parent && state.timestamp != 0 && state.time_shown != TimePlacement::Header)
            .map(|state| state.timestamp)
            .collect()
    });
    children.iter().copied().filter(|child| !moved.contains(child)).collect()
}

fn apply_author_tags(
    batch: &Batch,
    decorator: AstralDecorator,
    out: &mut Out,
    changed: &mut c_int,
    parent: i32,
    children: &[i32],
    message: u64,
) {
    let children = &without_moved_times(parent, children);
    let Some((tags, count)) = decorate_author(decorator, message) else {
        keep_tags(batch, out, changed, parent, children, message);
        return;
    };
    let tags = &tags[..count];
    with_decorated(|decorated| {
        let Some(slot) = decorated_slot(decorated, parent) else { return };
        slot.message = message;
        if tags.is_empty() && slot.nodes.is_empty() {
            slot.signature = tag_signature(&[]);
            return;
        }
        put_original_once(out, batch.bytes, changed);
        if tags.is_empty() {
            remove_tags(out, slot);
            slot.signature = tag_signature(&[]);
            out.put_children(parent, children);
            return;
        }
        if !(slot.roots.is_empty() && slot.trailing.is_empty()) && tag_signature(tags) == slot.signature {
            put_tagged_children(out, parent, children, slot);
            return;
        }
        write_tags(out, parent, children, tags, slot);
    });
}

fn batch_of_length<'a>(bytes: *const u8, pointer: *const Span, count: usize, length: usize) -> Batch<'a> {
    let bytes = if bytes.is_null() || length == 0 {
        &[][..]
    } else {
        unsafe { core::slice::from_raw_parts(bytes, length) }
    };
    Batch {
        bytes,
        spans: spans_of(pointer, count),
    }
}

#[unsafe(export_name = "wireDecorate")]
pub unsafe extern "C" fn wire_decorate(
    bytes: *const u8,
    pointer: *const Span,
    count: usize,
    out: *mut Out,
    changed: *mut c_int,
    length: usize,
) {
    let (Some(out), Some(changed)) = (unsafe { out.as_mut() }, unsafe { changed.as_mut() }) else {
        return;
    };
    let decorator = DECORATOR.get::<AstralDecorator>();
    if decorator.is_none() && BACKGROUND_DECORATOR.get::<AstralMessageBackgroundDecorator>().is_none() && wire_patching_active() == 0 {
        return;
    }
    let batch = batch_of_length(bytes, pointer, count, length);
    propagate_avatars(&batch);
    message_backgrounds(&batch, out, changed);
    if let Some(decorator) = decorator {
        decorate_tags(&batch, decorator, out, changed);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_wire_refresh_author_tags(buffer: *mut AstralWireBuffer) -> c_int {
    let Some(buffer) = (unsafe { buffer.as_mut() }) else { return 0 };
    *buffer = AstralWireBuffer {
        bytes: core::ptr::null_mut(),
        length: 0,
    };
    let Some(decorator) = DECORATOR.get::<AstralDecorator>() else {
        return 0;
    };
    let mut out = Out::batch();
    let mut any = false;
    let slots: Vec<(i32, u64)> = with_decorated(|decorated| {
        decorated
            .iter()
            .filter(|(parent, slot)| **parent != 0 && slot.message != 0)
            .map(|(parent, slot)| (*parent, slot.message))
            .collect()
    });
    for (parent, message) in slots {
        let children = without_moved_times(parent, &discord_children(parent));
        if children.is_empty() || children.len() > MAX_TAGGED_CHILDREN {
            continue;
        }
        let Some((tags, count)) = decorate_author(decorator, message) else {
            continue;
        };
        let tags = &tags[..count];
        with_decorated(|decorated| {
            let Some(slot) = decorated.get_mut(&parent) else { return };
            if tag_signature(tags) == slot.signature {
                return;
            }
            any = true;
            if tags.is_empty() {
                remove_tags(&mut out, slot);
                slot.signature = tag_signature(&[]);
                out.put_children(parent, &children);
                return;
            }
            write_tags(&mut out, parent, &children, tags, slot);
        });
    }
    if !any {
        out.discard();
        return 0;
    }
    c_int::from(out.finish(buffer))
}

#[unsafe(export_name = "wireObserveRemovals")]
pub unsafe extern "C" fn wire_observe_removals(
    bytes: *const u8,
    pointer: *const Span,
    count: usize,
    out: *mut Out,
    changed: *mut c_int,
    length: usize,
) {
    let (Some(out), Some(changed)) = (unsafe { out.as_mut() }, unsafe { changed.as_mut() }) else {
        return;
    };
    let batch = batch_of_length(bytes, pointer, count, length);
    for span in batch.spans.iter().filter(|span| span.op == Op::Remove) {
        for position in 0..span.child_count {
            let id = batch.child(span, position);
            if let Some(channel) = tracked_value(id, TRACK_CHANNEL)
                && current_channel() == channel
            {
                wire_set_current_channel(0);
            }
            with_tracked(|tracked| tracked.remove(&id));
            with_backgrounds(|backgrounds| {
                if let Some(state) = backgrounds.rows.get_mut(&id) {
                    state.active = false;
                    state.has_original = false;
                }
            });
            with_decorated(|decorated| {
                let Some(slot) = decorated.get_mut(&id) else { return };
                if !slot.nodes.is_empty() {
                    put_original_once(out, batch.bytes, changed);
                    remove_tags(out, slot);
                }
                slot.message = 0;
            });
        }
    }
}

#[unsafe(export_name = "wireObserve")]
pub unsafe extern "C" fn wire_observe(bytes: *const u8, pointer: *const Span, count: usize) {
    for span in spans_of(pointer, count) {
        if span.op == Op::Create || span.op == Op::Update {
            observe_properties(super::format::batch_of(bytes, span), span);
        }
    }
}
