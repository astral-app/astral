use super::tag::{Op, Tag};
use core::ffi::{CStr, c_char, c_int};
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicI32, Ordering};

use super::abi::{Callback, prefixed};
use super::clone::{Change, clone_creation};
use super::format::{
    Span, batch_of, child, find_string, property_cursor, spans_of, type_is, wire_child_list, wire_creation, wire_first_of_type,
    wire_parent_of,
};
use super::out::Out;
use super::properties;

#[repr(C)]
pub struct AstralMenuEntry {
    pub action_id: [c_char; 64],
    pub label: [c_char; 64],
    pub icon: [c_char; 160],
    pub danger: c_int,
}

pub type MenuProvider = unsafe extern "C" fn(*mut AstralMenuEntry, usize) -> usize;
pub type PopoverProvider = unsafe extern "C" fn(*const c_char, *mut AstralMenuEntry, usize) -> usize;

const LABEL_DEPTH: c_int = 4;
const MESSAGE_NODE_CAPACITY: usize = 32768;
const MAX_ENTRIES: usize = 12;
const MAX_SUBTREE: usize = 256;
const MAX_GROUP_CHILDREN: u32 = 64;
const MAX_LISTS: usize = 16;
const MAX_LIST_ROWS: u32 = 512;
const FIRST_MENU_ID: i32 = 0x7d00_0000;
const MENU_ID_RANGE: i32 = 0x00f0_0000;
const ACTION_PREFIX: &[u8] = b"message.actions.";
const POPOVER_TITLE: &[u8] = b"menu-title";
const POPOVER_ROW_PREFIX: &[u8] = b"menu-row-";
const POPOVER_FIRST_ROW: &[u8] = b"menu-row-0";
const POPOVER_MIDDLE_ROW: &[u8] = b"menu-row-1";
const SKIPPED_ACTIONS: [&[u8]; 3] = [b"quick_reaction", b"add_reaction", b"root"];
const MESSAGE_PREFIXES: [&CStr; 4] = [c"message.content:", c"message.author:", c"message.avatar:", c"message.timestamp:"];

#[derive(Clone, Default)]
struct ListWindow {
    list: i32,
    first: i64,
    count: i64,
    rows: Vec<i32>,
}

impl ListWindow {
    fn start(&self) -> i64 {
        let rows = self.rows.len() as i64;
        if self.count > 0 && self.first + rows > self.count {
            self.count - rows
        } else {
            self.first
        }
    }
}

#[derive(Default)]
struct Tracking {
    messages: HashMap<i32, u64>,
    lists: Vec<ListWindow>,
    next_list: usize,
}

static TRACKING: Mutex<Option<Tracking>> = Mutex::new(None);
static PROVIDER: Callback = Callback::new();
static POPOVER_PROVIDER: Callback = Callback::new();
static NEXT_MENU_ID: AtomicI32 = AtomicI32::new(FIRST_MENU_ID);

fn with_tracking<T>(work: impl FnOnce(&mut Tracking) -> T) -> Option<T> {
    let mut tracking = TRACKING.lock().ok()?;
    Some(work(tracking.get_or_insert_with(Tracking::default)))
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_wire_set_message_menu_provider(provider: Option<MenuProvider>) {
    PROVIDER.set(provider);
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_wire_set_popover_menu_provider(provider: Option<PopoverProvider>) {
    POPOVER_PROVIDER.set(provider);
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_wire_message_for_node(node: i32) -> u64 {
    with_tracking(|tracking| tracking.messages.get(&node).copied().unwrap_or(0)).unwrap_or(0)
}

fn message_of(bytes: &[u8], span: &Span) -> u64 {
    let Some(identifier) = find_string(bytes, span, properties::current().accessibility_id) else {
        return 0;
    };
    MESSAGE_PREFIXES.iter().find_map(|prefix| prefixed(identifier, prefix)).unwrap_or(0)
}

fn number_at(bytes: &[u8], span: &Span, wanted: u16) -> Option<i64> {
    let (mut cursor, count) = property_cursor(bytes, span);
    for _ in 0..count {
        if cursor.failed() {
            break;
        }
        let property = cursor.read16();
        let tag = cursor.read8();
        if property != wanted {
            cursor.skip_value(tag, 0);
            continue;
        }
        return match Tag::parse(tag) {
            Some(Tag::Int) => {
                let value = cursor.read32() as i32;
                (!cursor.failed()).then_some(i64::from(value))
            }
            Some(Tag::Long) => Some(i64::from_le_bytes(cursor.take(8)?.try_into().ok()?)),
            Some(Tag::Double) => Some(f64::from_le_bytes(cursor.take(8)?.try_into().ok()?) as i64),
            _ => None,
        };
    }
    None
}

fn window_index(tracking: &mut Tracking, bytes: &[u8], span: &Span) -> Option<usize> {
    let existing = tracking.lists.iter().position(|window| window.list == span.id);
    if span.op == Op::Update || span.op == Op::SetChildren {
        return existing;
    }
    if span.op != Op::Create || !type_is(bytes, span, b"list") {
        return None;
    }
    if existing.is_some() {
        return existing;
    }
    let window = ListWindow {
        list: span.id,
        ..ListWindow::default()
    };
    let slot = tracking.next_list % MAX_LISTS;
    tracking.next_list += 1;
    if slot < tracking.lists.len() {
        tracking.lists[slot] = window;
    } else {
        tracking.lists.push(window);
    }
    Some(slot.min(tracking.lists.len() - 1))
}

fn track_list_window(tracking: &mut Tracking, bytes: &[u8], span: &Span) {
    let properties = properties::current();
    let Some(index) = window_index(tracking, bytes, span) else { return };
    let window = &mut tracking.lists[index];
    if span.op == Op::SetChildren {
        window.rows = (0..span.child_count.min(MAX_LIST_ROWS))
            .map(|position| child(bytes, span, position))
            .collect();
        return;
    }
    if let Some(first) = number_at(bytes, span, properties.first) {
        window.first = first;
    }
    if let Some(count) = number_at(bytes, span, properties.count) {
        window.count = count;
    }
}

#[unsafe(export_name = "wireRowOfMessage")]
pub unsafe extern "C" fn wire_row_of_message(message: u64, list: *mut i32, index: *mut i64) -> c_int {
    let found = with_tracking(|tracking| {
        tracking.lists.iter().filter(|window| window.list != 0).find_map(|window| {
            let position = window.rows.iter().position(|row| tracking.messages.get(row) == Some(&message))?;
            Some((window.list, window.start() + position as i64))
        })
    })
    .flatten();
    let Some((found_list, found_index)) = found else { return 0 };
    if list.is_null() || index.is_null() {
        return 0;
    }
    unsafe {
        *list = found_list;
        *index = found_index;
    }
    1
}

fn message_for_row(tracking: &Tracking, list: i32, index: i64) -> u64 {
    let Some(window) = tracking.lists.iter().find(|window| window.list == list) else {
        return 0;
    };
    let start = window.start();
    if index < start {
        return 0;
    }
    window
        .rows
        .get((index - start) as usize)
        .and_then(|row| tracking.messages.get(row))
        .copied()
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_wire_message_for_row(list: i32, index: i64) -> u64 {
    with_tracking(|tracking| message_for_row(tracking, list, index)).unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_wire_message_at_row(index: i64) -> u64 {
    with_tracking(|tracking| {
        tracking
            .lists
            .iter()
            .filter(|window| window.list != 0)
            .map(|window| message_for_row(tracking, window.list, index))
            .find(|message| *message != 0)
    })
    .flatten()
    .unwrap_or(0)
}

struct Batch<'a> {
    bytes: *const u8,
    spans: &'a [Span],
}

impl Batch<'_> {
    fn list(&self, id: i32) -> Option<(&Span, &[u8])> {
        let list = unsafe { wire_child_list(self.bytes, self.spans.as_ptr(), self.spans.len(), id).as_ref() }?;
        Some((list, batch_of(self.bytes, list)))
    }

    fn children(&self, id: i32) -> Vec<i32> {
        self.list(id)
            .map(|(list, data)| (0..list.child_count).map(|position| child(data, list, position)).collect())
            .unwrap_or_default()
    }

    fn creation(&self, id: i32) -> Option<(&Span, &[u8])> {
        let creation = unsafe { wire_creation(self.spans.as_ptr(), self.spans.len(), id).as_ref() }?;
        Some((creation, batch_of(self.bytes, creation)))
    }

    fn parent(&self, id: i32) -> i32 {
        unsafe { wire_parent_of(self.bytes, self.spans.as_ptr(), self.spans.len(), id) }
    }

    fn identifier(&self, id: i32) -> Option<&[u8]> {
        let (creation, data) = self.creation(id)?;
        find_string(data, creation, properties::current().accessibility_id)
    }
}

fn subtree(batch: &Batch, root: i32) -> Vec<i32> {
    let mut nodes = vec![root];
    let mut cursor = 0;
    while cursor < nodes.len() && nodes.len() < MAX_SUBTREE {
        let children = batch.children(nodes[cursor]);
        cursor += 1;
        for id in children {
            if nodes.len() >= MAX_SUBTREE {
                break;
            }
            nodes.push(id);
        }
    }
    nodes
}

#[unsafe(export_name = "wireTrackMessageNodes")]
pub unsafe extern "C" fn wire_track_message_nodes(bytes: *const u8, pointer: *const Span, count: usize) {
    let batch = Batch {
        bytes,
        spans: spans_of(pointer, count),
    };
    let accessibility = properties::current().accessibility_id;
    with_tracking(|tracking| {
        for span in batch.spans {
            let data = batch_of(bytes, span);
            track_list_window(tracking, data, span);
            if span.op != Op::Create || find_string(data, span, accessibility) != Some(b"message-row") {
                continue;
            }
            let nodes = subtree(&batch, span.id);
            let message = nodes.iter().find_map(|node| {
                batch
                    .creation(*node)
                    .map(|(creation, data)| message_of(data, creation))
                    .filter(|message| *message != 0)
            });
            let Some(message) = message else { continue };
            if tracking.messages.len() > MESSAGE_NODE_CAPACITY {
                tracking.messages.clear();
            }
            for node in nodes {
                tracking.messages.insert(node, message);
            }
        }
    });
}

fn menu_id() -> i32 {
    let id = NEXT_MENU_ID.fetch_add(1, Ordering::Relaxed);
    if id + 1 >= FIRST_MENU_ID + MENU_ID_RANGE {
        NEXT_MENU_ID.store(FIRST_MENU_ID, Ordering::Relaxed);
    }
    id
}

fn is_action_row(bytes: &[u8], span: &Span) -> bool {
    if span.op != Op::Create {
        return false;
    }
    let Some(identifier) = find_string(bytes, span, properties::current().accessibility_id) else {
        return false;
    };
    let Some(action) = identifier.strip_prefix(ACTION_PREFIX) else {
        return false;
    };
    !action.is_empty() && !SKIPPED_ACTIONS.iter().any(|skipped| action.starts_with(skipped))
}

fn clone_node(out: &mut Out, batch: &Batch, source: i32, changes: &[Change]) -> i32 {
    let Some((creation, data)) = (source != 0).then(|| batch.creation(source)).flatten() else {
        return 0;
    };
    let id = menu_id();
    clone_creation(out, data, creation, id, changes);
    id
}

fn put_children(out: &mut Out, parent: i32, children: &[i32]) {
    out.put_op(Op::SetChildren);
    out.put(&parent.to_le_bytes());
    out.put32(children.len() as u32);
    for id in children {
        out.put(&id.to_le_bytes());
    }
}

fn clone_tree(out: &mut Out, batch: &Batch, source: i32, depth: u32) -> i32 {
    let id = clone_node(out, batch, source, &[]);
    if id == 0 || depth >= 4 {
        return id;
    }
    let children: Vec<i32> = batch
        .children(source)
        .into_iter()
        .take(16)
        .map(|id| clone_tree(out, batch, id, depth + 1))
        .filter(|id| *id != 0)
        .collect();
    if !children.is_empty() {
        put_children(out, id, &children);
    }
    id
}

fn entry_text(text: &[c_char]) -> Vec<u8> {
    let length = text.iter().position(|byte| *byte == 0).unwrap_or(text.len());
    text[..length].iter().map(|byte| *byte as u8).collect()
}

fn build_row(out: &mut Out, batch: &Batch, template: i32, entry: &AstralMenuEntry, first: bool) -> i32 {
    let properties = properties::current();
    let Some(content) = batch.children(template).first().copied() else {
        return 0;
    };
    let parts = batch.children(content);
    if parts.len() < 2 {
        return 0;
    }
    let icon = parts[0];
    let label = unsafe {
        wire_first_of_type(
            batch.bytes,
            batch.spans.as_ptr(),
            batch.spans.len(),
            parts[1],
            c"text".as_ptr(),
            LABEL_DEPTH,
        )
    };
    let (action, title, image) = (entry_text(&entry.action_id), entry_text(&entry.label), entry_text(&entry.icon));
    let corner = |property: u16| {
        if first {
            Change::number(property, 12.0)
        } else {
            Change::drop(property)
        }
    };
    let row_changes = [
        Change::text(properties.accessibility_id, &action),
        Change::text(properties.accessibility_label, &title),
        corner(properties.radius_top_left),
        corner(properties.radius_top_right),
        Change::drop(properties.radius_bottom_left),
        Change::drop(properties.radius_bottom_right),
    ];
    let icon_changes = if image.is_empty() {
        Vec::new()
    } else {
        vec![Change::text(properties.uri, &image)]
    };
    let row = clone_node(out, batch, template, &row_changes);
    let row_content = clone_node(out, batch, content, &[]);
    let row_icon = clone_node(out, batch, icon, &icon_changes);
    let row_label = clone_node(out, batch, label, &[Change::text(properties.text, &title)]);
    if row == 0 || row_content == 0 || row_icon == 0 || row_label == 0 {
        return 0;
    }
    put_children(out, row, &[row_content]);
    put_children(out, row_content, &[row_icon, row_label]);
    row
}

fn action_row(batch: &Batch, wanted: &[u8]) -> Option<i32> {
    batch.spans.iter().find_map(|span| {
        let data = batch_of(batch.bytes, span);
        (is_action_row(data, span) && find_string(data, span, properties::current().accessibility_id) == Some(wanted)).then_some(span.id)
    })
}

fn template_row(batch: &Batch) -> i32 {
    let mut fallback = 0;
    for span in batch.spans {
        let data = batch_of(batch.bytes, span);
        if !is_action_row(data, span) {
            continue;
        }
        let identifier = find_string(data, span, properties::current().accessibility_id);
        if identifier == Some(b"message.actions.copy_text") {
            return span.id;
        }
        if fallback == 0 && identifier != Some(b"message.actions.delete") {
            fallback = span.id;
        }
    }
    fallback
}

fn separator_in(batch: &Batch, children: &[i32]) -> i32 {
    children
        .iter()
        .copied()
        .find(|id| batch.identifier(*id).is_none_or(<[u8]>::is_empty) && batch.creation(*id).is_some())
        .unwrap_or(0)
}

fn empty_entries() -> Vec<AstralMenuEntry> {
    (0..MAX_ENTRIES)
        .map(|_| AstralMenuEntry {
            action_id: [0; 64],
            label: [0; 64],
            icon: [0; 160],
            danger: 0,
        })
        .collect()
}

fn menu_entries() -> Vec<AstralMenuEntry> {
    let Some(provider) = PROVIDER.get::<MenuProvider>() else {
        return Vec::new();
    };
    let mut entries = empty_entries();
    let count = unsafe { provider(entries.as_mut_ptr(), MAX_ENTRIES) }.min(MAX_ENTRIES);
    entries.truncate(count);
    entries
}

fn popover_entries(title: &[u8]) -> Vec<AstralMenuEntry> {
    let Some(provider) = POPOVER_PROVIDER.get::<PopoverProvider>() else {
        return Vec::new();
    };
    let Ok(title) = std::ffi::CString::new(title) else {
        return Vec::new();
    };
    let mut entries = empty_entries();
    let count = unsafe { provider(title.as_ptr(), entries.as_mut_ptr(), MAX_ENTRIES) }.min(MAX_ENTRIES);
    entries.truncate(count);
    entries
}

fn popover_row(out: &mut Out, batch: &Batch, template: i32, entry: &AstralMenuEntry) -> i32 {
    let properties = properties::current();
    let parts = batch.children(template);
    let [icon, column] = parts.as_slice() else { return 0 };
    let Some(label) = batch.children(*column).first().copied() else {
        return 0;
    };
    let (action, title, image) = (entry_text(&entry.action_id), entry_text(&entry.label), entry_text(&entry.icon));
    let row = clone_node(
        out,
        batch,
        template,
        &[
            Change::text(properties.accessibility_id, &action),
            Change::text(properties.accessibility_label, &title),
        ],
    );
    let icon_changes = if image.is_empty() {
        Vec::new()
    } else {
        vec![Change::text(properties.uri, &image)]
    };
    let row_icon = clone_node(out, batch, *icon, &icon_changes);
    let row_column = clone_node(out, batch, *column, &[]);
    let row_label = clone_node(out, batch, label, &[Change::text(properties.text, &title)]);
    if row == 0 || row_icon == 0 || row_column == 0 || row_label == 0 {
        return 0;
    }
    put_children(out, row_column, &[row_label]);
    put_children(out, row, &[row_icon, row_column]);
    row
}

#[unsafe(export_name = "wirePopoverMenu")]
pub unsafe extern "C" fn wire_popover_menu(
    bytes: *const u8,
    pointer: *const Span,
    count: usize,
    out: *mut Out,
    changed: *mut c_int,
    length: usize,
) {
    if POPOVER_PROVIDER.get::<PopoverProvider>().is_none() {
        return;
    }
    let (Some(out), Some(changed)) = (unsafe { out.as_mut() }, unsafe { changed.as_mut() }) else {
        return;
    };
    let batch = Batch {
        bytes,
        spans: spans_of(pointer, count),
    };
    let properties = properties::current();
    let Some(title_span) = batch.spans.iter().find(|span| {
        let data = batch_of(bytes, span);
        span.op == Op::Create && find_string(data, span, properties.accessibility_id) == Some(POPOVER_TITLE)
    }) else {
        return;
    };
    let title = find_string(batch_of(bytes, title_span), title_span, properties.text).unwrap_or_default();
    let list = batch.parent(title_span.id);
    let children = batch.children(list);
    let template = [POPOVER_MIDDLE_ROW, POPOVER_FIRST_ROW]
        .iter()
        .find_map(|wanted| children.iter().copied().find(|id| batch.identifier(*id) == Some(*wanted)));
    let Some(template) = template else { return };
    let Some(first_row) = children
        .iter()
        .position(|id| batch.identifier(*id).is_some_and(|id| id.starts_with(POPOVER_ROW_PREFIX)))
    else {
        return;
    };
    let entries = popover_entries(title);
    if entries.is_empty() {
        return;
    }
    let mut update = Out::default();
    let rows: Vec<i32> = entries
        .iter()
        .map(|entry| popover_row(&mut update, &batch, template, entry))
        .take_while(|row| *row != 0)
        .collect();
    if update.failed != 0 || rows.is_empty() {
        update.discard();
        return;
    }
    let mut ordered = children[..first_row].to_vec();
    ordered.extend(rows);
    ordered.extend_from_slice(&children[first_row..]);
    put_children(&mut update, list, &ordered);
    if *changed == 0 {
        out.put(unsafe { core::slice::from_raw_parts(bytes, length) });
    }
    *changed = 1;
    out.put(unsafe { core::slice::from_raw_parts(update.bytes, update.length) });
    update.discard();
}

#[unsafe(export_name = "wireMessageMenu")]
pub unsafe extern "C" fn wire_message_menu(
    bytes: *const u8,
    pointer: *const Span,
    count: usize,
    out: *mut Out,
    changed: *mut c_int,
    length: usize,
) {
    if PROVIDER.get::<MenuProvider>().is_none() {
        return;
    }
    let (Some(out), Some(changed)) = (unsafe { out.as_mut() }, unsafe { changed.as_mut() }) else {
        return;
    };
    let batch = Batch {
        bytes,
        spans: spans_of(pointer, count),
    };
    let template = template_row(&batch);
    let danger_template = action_row(&batch, b"message.actions.delete").unwrap_or(template);
    let group = batch
        .spans
        .iter()
        .find(|span| is_action_row(batch_of(bytes, span), span))
        .map_or(0, |span| batch.parent(span.id));
    let group_children = if group != 0 { batch.children(group) } else { Vec::new() };
    if template == 0 || group_children.is_empty() || group_children.len() > MAX_GROUP_CHILDREN as usize {
        return;
    }
    let entries = menu_entries();
    if entries.is_empty() {
        return;
    }
    let mut separator = separator_in(&batch, &group_children);
    if separator == 0 {
        separator = separator_in(&batch, &batch.children(batch.parent(template)));
    }
    let mut children = Vec::new();
    let mut update = Out::default();
    for (index, entry) in entries.iter().enumerate() {
        let source = if entry.danger != 0 { danger_template } else { template };
        let row = build_row(&mut update, &batch, source, entry, index == 0);
        if row == 0 {
            break;
        }
        children.push(row);
        let divider = if separator != 0 {
            clone_tree(&mut update, &batch, separator, 0)
        } else {
            0
        };
        if divider != 0 {
            children.push(divider);
        }
    }
    if update.failed != 0 || children.is_empty() {
        update.discard();
        return;
    }
    let first_original = group_children[0];
    if batch.identifier(first_original).is_some_and(|identifier| !identifier.is_empty()) {
        let properties = properties::current();
        update.update(first_original, 2);
        update.prop_number(properties.radius_top_left, 0.0);
        update.prop_number(properties.radius_top_right, 0.0);
    }
    children.extend(group_children);
    put_children(&mut update, group, &children);
    if *changed == 0 {
        out.put(unsafe { core::slice::from_raw_parts(bytes, length) });
    }
    *changed = 1;
    out.put(unsafe { core::slice::from_raw_parts(update.bytes, update.length) });
    update.discard();
}
