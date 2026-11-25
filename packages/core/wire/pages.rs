use core::ffi::{CStr, c_char, c_int};
use std::ffi::CString;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::clone::{Change, Edit, clone_creation};
use super::format::{Span, batch_of, child, find_string, spans_of, type_is, wireChildList, wireCreation, wireFirstOfType, wireParentOf};
use super::out::{AstralWireBuffer, Out};
use super::properties::{self, AstralWireProperties};

const TITLE_DEPTH: c_int = 4;
const ANCHOR: &[u8] = b"settings.row.account";
const MAX_CATEGORY_NODES: usize = 128;
const MAX_CATEGORY_ROWS: usize = 8;
const CATEGORY_TEXT: usize = 96;
const FIRST_ASTRAL_ID: i32 = 0x7a000000;
const FIRST_PAGE_ID: i32 = 0x7a000100;
const PAGE_ID_STRIDE: i32 = 0x40000;
const MAX_NAVIGATOR_CHILDREN: usize = 64;
const MAX_PAGE_NODES: usize = 1024;
const MAX_NAVIGATORS: usize = 8;
const MAX_PAGES: usize = 4;
const MAX_CHILDREN: usize = 4096;
const SWITCH_ON: u32 = 0xFF5865F2;
const SWITCH_OFF: i32 = 0x1F000000;
const THUMB: u32 = 0xFFFFFFFF;
const SWITCH_BORDER: i32 = 530028447;
const SLIDER_ACTIVE: u32 = 0xFF5865F2;
const SLIDER_INACTIVE: u32 = 0xFF4E5058;
const TRANSLUCENT_FILL: u32 = 0x1FFFFFFF;
const SUBTLE_BORDER: u32 = 0x1F97979F;
const CRITICAL_TEXT: u32 = 0xFFF23F43;

const ROW_LINK: c_int = 0;
const ROW_SWITCH: c_int = 1;
const ROW_INFO: c_int = 2;
const ROW_SWITCH_LINK: c_int = 3;
const ROW_SELECTED: c_int = 4;
const ROW_NUMBER: c_int = 6;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Root,
    Phone,
    Column,
    Container,
    Spacer,
    Hairline,
    Scroll,
    List,
    HeaderBar,
    HeaderLeft,
    HeaderLeftInner,
    HeaderLeftPad,
    Back,
    BackStack,
    BackStackInner,
    BackIcon,
    TitleWrap,
    TitleInner,
    Title,
    Group,
    GroupHeader,
    Card,
    Row,
    RowContent,
    RowIcon,
    RowTitle,
    RowTrailing,
    RowChevron,
    Separator,
    SeparatorLine,
}

const ROLE_COUNT: usize = Role::SeparatorLine as usize + 1;

#[repr(C)]
pub struct AstralRowSpec {
    pub key: *const c_char,
    pub title: *const c_char,
    pub subtitle: *const c_char,
    pub detail: *const c_char,
    pub icon: *const c_char,
    pub kind: c_int,
    pub on: c_int,
    pub steps: i32,
    pub position: i32,
}

#[repr(C)]
pub struct AstralSectionSpec {
    pub title: *const c_char,
    pub rows: *const AstralRowSpec,
    pub count: usize,
}

#[repr(C)]
pub struct AstralChipSpec {
    pub key: *const c_char,
    pub title: *const c_char,
    pub selected: c_int,
}

#[repr(C)]
pub struct AstralToolbarSpec {
    pub chips: *const AstralChipSpec,
    pub chip_count: usize,
    pub button_key: *const c_char,
    pub button_icon: *const c_char,
}

#[repr(C)]
pub struct AstralCardSpec {
    pub key: *const c_char,
    pub title: *const c_char,
    pub authors: *const c_char,
    pub description: *const c_char,
    pub version: *const c_char,
    pub icon: *const c_char,
    pub error: *const c_char,
    pub avatars: [*const c_char; 3],
    pub avatar_count: usize,
    pub has_settings: c_int,
    pub on: c_int,
    pub failed: c_int,
}

#[repr(C)]
pub struct AstralPageSpec {
    pub title: *const c_char,
    pub sections: *const AstralSectionSpec,
    pub count: usize,
    pub toolbar: *const AstralToolbarSpec,
    pub cards: *const AstralCardSpec,
    pub card_count: usize,
}

fn text(pointer: *const c_char) -> Option<Vec<u8>> {
    (!pointer.is_null()).then(|| unsafe { CStr::from_ptr(pointer) }.to_bytes().to_vec())
}

fn text_or_empty(pointer: *const c_char) -> Vec<u8> {
    text(pointer).unwrap_or_default()
}

fn list<'a, T>(pointer: *const T, count: usize) -> &'a [T] {
    if pointer.is_null() || count == 0 {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(pointer, count) }
    }
}

struct Row {
    key: Vec<u8>,
    title: Vec<u8>,
    subtitle: Vec<u8>,
    detail: Vec<u8>,
    icon: Option<Vec<u8>>,
    kind: c_int,
    on: bool,
    steps: i32,
    position: i32,
}

struct Section {
    title: Vec<u8>,
    rows: Vec<Row>,
}

struct Chip {
    key: Vec<u8>,
    title: Vec<u8>,
    selected: bool,
}

struct Toolbar {
    chips: Vec<Chip>,
    button: Option<(Vec<u8>, Vec<u8>)>,
}

struct Card {
    key: Vec<u8>,
    title: Vec<u8>,
    authors: Vec<u8>,
    description: Vec<u8>,
    version: Vec<u8>,
    icon: Option<Vec<u8>>,
    error: Vec<u8>,
    avatars: Vec<Vec<u8>>,
    has_settings: bool,
    on: bool,
    failed: bool,
}

struct PageSpec {
    title: Vec<u8>,
    sections: Vec<Section>,
    toolbar: Option<Toolbar>,
    cards: Vec<Card>,
}

fn row_of(spec: &AstralRowSpec) -> Row {
    Row {
        key: text_or_empty(spec.key),
        title: text_or_empty(spec.title),
        subtitle: text_or_empty(spec.subtitle),
        detail: text_or_empty(spec.detail),
        icon: text(spec.icon),
        kind: spec.kind,
        on: spec.on != 0,
        steps: spec.steps,
        position: spec.position,
    }
}

fn page_of(spec: &AstralPageSpec) -> PageSpec {
    let toolbar = unsafe { spec.toolbar.as_ref() }.map(|toolbar| Toolbar {
        chips: list(toolbar.chips, toolbar.chip_count)
            .iter()
            .map(|chip| Chip {
                key: text_or_empty(chip.key),
                title: text_or_empty(chip.title),
                selected: chip.selected != 0,
            })
            .collect(),
        button: text(toolbar.button_key).map(|key| (key, text_or_empty(toolbar.button_icon))),
    });
    PageSpec {
        title: text_or_empty(spec.title),
        sections: list(spec.sections, spec.count)
            .iter()
            .map(|section| Section {
                title: text_or_empty(section.title),
                rows: list(section.rows, section.count).iter().map(row_of).collect(),
            })
            .collect(),
        toolbar,
        cards: list(spec.cards, spec.card_count)
            .iter()
            .map(|card| Card {
                key: text_or_empty(card.key),
                title: text_or_empty(card.title),
                authors: text_or_empty(card.authors),
                description: text_or_empty(card.description),
                version: text_or_empty(card.version),
                icon: text(card.icon),
                error: text_or_empty(card.error),
                avatars: card.avatars[..card.avatar_count.min(3)]
                    .iter()
                    .map(|avatar| text_or_empty(*avatar))
                    .collect(),
                has_settings: card.has_settings != 0,
                on: card.on != 0,
                failed: card.failed != 0,
            })
            .collect(),
    }
}

#[derive(Clone, Default)]
struct Navigator {
    id: i32,
    children: Vec<i32>,
    index: i32,
}

#[derive(Default)]
struct Page {
    ids: Vec<i32>,
    first_id: i32,
    next_id: i32,
    list: i32,
    content_start: usize,
    content_end: usize,
    previous_children: Vec<i32>,
    previous_index: i32,
}

struct CategoryRow {
    key: Vec<u8>,
    title: Vec<u8>,
    icon: Vec<u8>,
}

struct State {
    category_list: i32,
    category_ids: Vec<i32>,
    category_active: bool,
    category_title: Vec<u8>,
    category_rows: Vec<CategoryRow>,
    batch_length: usize,
    templates: Vec<Option<Vec<u8>>>,
    templates_ready: bool,
    header_has_pad: bool,
    title_has_inner: bool,
    template_failure: String,
    navigators: Vec<Navigator>,
    settings_navigator: i32,
    pages: Vec<Page>,
}

impl State {
    fn new() -> Self {
        State {
            category_list: 0,
            category_ids: Vec::new(),
            category_active: false,
            category_title: b"astral".to_vec(),
            category_rows: vec![CategoryRow {
                key: Vec::new(),
                title: b"astral".to_vec(),
                icon: b"res://ic_mana_sparkles".to_vec(),
            }],
            batch_length: 0,
            templates: vec![None; ROLE_COUNT],
            templates_ready: false,
            header_has_pad: false,
            title_has_inner: false,
            template_failure: "no settings batch seen".into(),
            navigators: Vec::new(),
            settings_navigator: 0,
            pages: Vec::new(),
        }
    }

    fn navigator(&mut self, id: i32) -> Option<&mut Navigator> {
        self.navigators.iter_mut().find(|navigator| navigator.id == id)
    }
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static TEMPLATE_SOURCE: AtomicUsize = AtomicUsize::new(0);
static STATUS: Mutex<Option<CString>> = Mutex::new(None);

fn with_state<T>(work: impl FnOnce(&mut State) -> T) -> Option<T> {
    let mut state = STATE.lock().ok()?;
    Some(work(state.get_or_insert_with(State::new)))
}

fn clipped(value: Vec<u8>) -> Vec<u8> {
    value.into_iter().take(CATEGORY_TEXT - 1).collect()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_wire_set_category(title: *const c_char, rows: *const AstralRowSpec, count: usize) {
    let title = clipped(text_or_empty(title));
    let rows: Vec<CategoryRow> = list(rows, count.min(MAX_CATEGORY_ROWS))
        .iter()
        .map(|row| CategoryRow {
            key: clipped(text_or_empty(row.key)),
            title: clipped(text_or_empty(row.title)),
            icon: clipped(text_or_empty(row.icon)),
        })
        .collect();
    with_state(|state| {
        state.category_title = title;
        state.category_rows = rows;
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn wireSetTemplateSource(raw: *const u8) {
    TEMPLATE_SOURCE.store(raw as usize, Ordering::Relaxed);
}

struct Batch<'a> {
    bytes: *const u8,
    spans: &'a [Span],
}

impl Batch<'_> {
    fn parent(&self, id: i32) -> i32 {
        if id == 0 {
            0
        } else {
            unsafe { wireParentOf(self.bytes, self.spans.as_ptr(), self.spans.len(), id) }
        }
    }

    fn children(&self, id: i32) -> Vec<i32> {
        let Some(list) = (id != 0)
            .then(|| unsafe { wireChildList(self.bytes, self.spans.as_ptr(), self.spans.len(), id).as_ref() })
            .flatten()
        else {
            return Vec::new();
        };
        let data = batch_of(self.bytes, list);
        (0..list.child_count).map(|position| child(data, list, position)).collect()
    }

    fn child_at(&self, id: i32, position: usize) -> i32 {
        self.children(id).get(position).copied().unwrap_or(0)
    }

    fn creation(&self, id: i32) -> Option<&Span> {
        (id != 0)
            .then(|| unsafe { wireCreation(self.spans.as_ptr(), self.spans.len(), id).as_ref() })
            .flatten()
    }

    fn first_text(&self, id: i32) -> i32 {
        unsafe { wireFirstOfType(self.bytes, self.spans.as_ptr(), self.spans.len(), id, c"text".as_ptr(), TITLE_DEPTH) }
    }
}

fn put_children(out: &mut Out, parent: i32, children: &[i32]) {
    out.put8(3);
    out.put(&parent.to_le_bytes());
    out.put32(children.len() as u32);
    for id in children {
        out.put(&id.to_le_bytes());
    }
}

fn put_removal(out: &mut Out, ids: &[i32]) {
    out.put8(4);
    out.put32(ids.len() as u32);
    for id in ids {
        out.put(&id.to_le_bytes());
    }
}

fn put_list_with_astral(out: &mut Out, state: &State, batch: &Batch, list: i32) {
    let first = state.category_ids.first().copied().unwrap_or(0);
    let mut children = Vec::new();
    for (index, id) in batch.children(list).into_iter().enumerate() {
        if children.len() >= MAX_CHILDREN - 1 {
            break;
        }
        if id == first {
            continue;
        }
        children.push(id);
        if index == 0 {
            children.push(first);
        }
    }
    put_children(out, list, &children);
}

fn corner(property: u16, rounded: bool) -> Change {
    if rounded {
        Change::number(property, 12.0)
    } else {
        Change::drop(property)
    }
}

fn category_node(out: &mut Out, state: &mut State, batch: &Batch, source: i32, changes: &[Change]) -> i32 {
    let Some(span) = batch.creation(source).filter(|_| state.category_ids.len() < MAX_CATEGORY_NODES) else {
        out.failed = 1;
        return 0;
    };
    let id = FIRST_ASTRAL_ID + state.category_ids.len() as i32;
    state.category_ids.push(id);
    clone_creation(out, batch_of(batch.bytes, span), span, id, changes);
    id
}

fn category_row(
    out: &mut Out,
    state: &mut State,
    batch: &Batch,
    source: &[i32; 6],
    index: usize,
    properties: &AstralWireProperties,
) -> i32 {
    let row = &state.category_rows[index];
    let identifier = if row.key.is_empty() {
        b"settings.row.astral".to_vec()
    } else {
        [b"settings.row.astral.".as_slice(), &row.key].concat()
    };
    let (title, icon) = (row.title.clone(), row.icon.clone());
    let (first, last) = (index == 0, index + 1 == state.category_rows.len());
    let row_changes = [
        Change::text(properties.accessibilityId, &identifier),
        Change::text(properties.accessibilityLabel, &title),
        corner(properties.radiusBottomLeft, last),
        corner(properties.radiusBottomRight, last),
        corner(properties.radiusTopLeft, first),
        corner(properties.radiusTopRight, first),
    ];
    let row_id = category_node(out, state, batch, source[0], &row_changes);
    let content = category_node(out, state, batch, source[1], &[]);
    let icon_id = category_node(out, state, batch, source[2], &[Change::text(properties.uri, &icon)]);
    let title_id = category_node(
        out,
        state,
        batch,
        source[3],
        &[Change::text(properties.text, &title), Change::number(properties.flex, 1.0)],
    );
    let trailing = category_node(out, state, batch, source[4], &[]);
    let chevron = category_node(out, state, batch, source[5], &[]);
    put_children(out, row_id, &[content]);
    put_children(out, content, &[icon_id, title_id, trailing]);
    put_children(out, trailing, &[chevron]);
    row_id
}

fn create_category(out: &mut Out, state: &mut State, batch: &Batch, row: i32) -> bool {
    let properties = properties::current();
    let card = batch.parent(row);
    let group = batch.parent(card);
    let list = batch.parent(group);
    let group_children = batch.children(group);
    let card_children = batch.children(card);
    let row_children = batch.children(row);
    if list == 0 || group_children.len() < 2 || row_children.is_empty() || card_children.is_empty() {
        return false;
    }
    let header = group_children[0];
    let content = row_children[0];
    let content_children = batch.children(content);
    if content_children.len() < 3 {
        return false;
    }
    let trailing = content_children[2];
    let Some(chevron) = batch.children(trailing).first().copied() else {
        return false;
    };
    let source = [
        row,
        content,
        content_children[0],
        batch.first_text(content_children[1]),
        trailing,
        chevron,
    ];
    let separator = card_children.get(1).copied().unwrap_or(0);
    let separator_line = batch.child_at(separator, 0);
    if state.category_rows.len() > 1 && (separator == 0 || separator_line == 0) {
        return false;
    }
    state.category_ids.clear();
    let title = state.category_title.clone();
    let new_group = category_node(out, state, batch, group, &[]);
    let new_header = category_node(out, state, batch, header, &[Change::text(properties.text, &title)]);
    let new_card = category_node(out, state, batch, card, &[]);
    let mut children = Vec::new();
    for index in 0..state.category_rows.len() {
        if index > 0 {
            let new_separator = category_node(out, state, batch, separator, &[]);
            let new_line = category_node(out, state, batch, separator_line, &[]);
            put_children(out, new_separator, &[new_line]);
            children.push(new_separator);
        }
        children.push(category_row(out, state, batch, &source, index, &properties));
    }
    if out.failed != 0 {
        return false;
    }
    put_children(out, new_group, &[new_header, new_card]);
    put_children(out, new_card, &children);
    state.category_list = list;
    state.category_active = true;
    put_list_with_astral(out, state, batch, list);
    true
}

fn store_template(state: &mut State, batch: &Batch, role: Role, id: i32) -> bool {
    let Some(span) = batch.creation(id) else { return false };
    let source = TEMPLATE_SOURCE.load(Ordering::Relaxed) as *const u8;
    let bytes = if source.is_null() { batch.bytes } else { source };
    let data = batch_of(bytes, span);
    state.templates[role as usize] = Some(data[span.start..span.end].to_vec());
    true
}

fn capture_templates(state: &mut State, batch: &Batch, row: i32) {
    let mut ids = [0i32; ROLE_COUNT];
    let set = |ids: &mut [i32; ROLE_COUNT], role: Role, id: i32| ids[role as usize] = id;
    let get = |ids: &[i32; ROLE_COUNT], role: Role| ids[role as usize];
    set(&mut ids, Role::Row, row);
    for (role, from) in [
        (Role::Card, Role::Row),
        (Role::Group, Role::Card),
        (Role::List, Role::Group),
        (Role::Scroll, Role::List),
        (Role::Container, Role::Scroll),
        (Role::Column, Role::Container),
        (Role::Phone, Role::Column),
        (Role::Root, Role::Phone),
    ] {
        let parent = batch.parent(get(&ids, from));
        set(&mut ids, role, parent);
    }
    let container = get(&ids, Role::Container);
    set(&mut ids, Role::Spacer, batch.child_at(container, 0));
    set(&mut ids, Role::Hairline, batch.child_at(container, 1));
    if batch.child_at(container, 2) != get(&ids, Role::Scroll) || get(&ids, Role::Scroll) == 0 {
        state.template_failure = if get(&ids, Role::Scroll) != 0 {
            "the scroll view is not the container's third child"
        } else {
            "the row's ancestors do not reach a scroll view"
        }
        .into();
        return;
    }
    let first = |ids: &[i32; ROLE_COUNT], role: Role| batch.child_at(ids[role as usize], 0);
    set(&mut ids, Role::HeaderBar, batch.child_at(container, 3));
    let value = first(&ids, Role::HeaderBar);
    set(&mut ids, Role::HeaderLeft, value);
    let value = first(&ids, Role::HeaderLeft);
    set(&mut ids, Role::HeaderLeftInner, value);
    let value = first(&ids, Role::HeaderLeftInner);
    set(&mut ids, Role::HeaderLeftPad, value);
    let value = first(&ids, Role::HeaderLeftPad);
    set(&mut ids, Role::Back, value);
    let value = first(&ids, Role::Back);
    set(&mut ids, Role::BackStack, value);
    let value = first(&ids, Role::BackStack);
    set(&mut ids, Role::BackStackInner, value);
    let value = first(&ids, Role::BackStackInner);
    set(&mut ids, Role::BackIcon, value);
    let has_pad = get(&ids, Role::BackIcon) != 0;
    if !has_pad {
        let value = get(&ids, Role::HeaderLeftPad);
        set(&mut ids, Role::Back, value);
        set(&mut ids, Role::HeaderLeftPad, 0);
        let value = first(&ids, Role::Back);
        set(&mut ids, Role::BackStack, value);
        let value = first(&ids, Role::BackStack);
        set(&mut ids, Role::BackStackInner, value);
        let value = first(&ids, Role::BackStackInner);
        set(&mut ids, Role::BackIcon, value);
    }
    let value = batch.child_at(get(&ids, Role::HeaderBar), 1);
    set(&mut ids, Role::TitleWrap, value);
    let value = first(&ids, Role::TitleWrap);
    set(&mut ids, Role::TitleInner, value);
    let value = first(&ids, Role::TitleInner);
    set(&mut ids, Role::Title, value);
    let has_inner = get(&ids, Role::Title) != 0;
    if !has_inner {
        let value = get(&ids, Role::TitleInner);
        set(&mut ids, Role::Title, value);
        set(&mut ids, Role::TitleInner, 0);
    }
    let value = first(&ids, Role::Group);
    set(&mut ids, Role::GroupHeader, value);
    let value = batch.child_at(get(&ids, Role::Card), 1);
    set(&mut ids, Role::Separator, value);
    let value = first(&ids, Role::Separator);
    set(&mut ids, Role::SeparatorLine, value);
    set(&mut ids, Role::RowContent, batch.child_at(row, 0));
    let value = first(&ids, Role::RowContent);
    set(&mut ids, Role::RowIcon, value);
    let value = batch.first_text(batch.child_at(get(&ids, Role::RowContent), 1));
    set(&mut ids, Role::RowTitle, value);
    let value = batch.child_at(get(&ids, Role::RowContent), 2);
    set(&mut ids, Role::RowTrailing, value);
    let value = first(&ids, Role::RowTrailing);
    set(&mut ids, Role::RowChevron, value);
    let navigator = batch.parent(get(&ids, Role::Root));
    if navigator == 0 || state.navigator(navigator).is_none() {
        state.template_failure = if navigator != 0 {
            "the settings root's parent is not a known navigator"
        } else {
            "the settings root has no parent"
        }
        .into();
        return;
    }
    for (index, id) in ids.iter().enumerate() {
        if (index == Role::HeaderLeftPad as usize && !has_pad) || (index == Role::TitleInner as usize && !has_inner) {
            continue;
        }
        let role = ROLES[index];
        if !store_template(state, batch, role, *id) {
            state.template_failure = format!("settings template role {index} was missing");
            return;
        }
    }
    state.template_failure.clear();
    state.settings_navigator = navigator;
    state.header_has_pad = has_pad;
    state.title_has_inner = has_inner;
    state.templates_ready = true;
}

const ROLES: [Role; ROLE_COUNT] = [
    Role::Root,
    Role::Phone,
    Role::Column,
    Role::Container,
    Role::Spacer,
    Role::Hairline,
    Role::Scroll,
    Role::List,
    Role::HeaderBar,
    Role::HeaderLeft,
    Role::HeaderLeftInner,
    Role::HeaderLeftPad,
    Role::Back,
    Role::BackStack,
    Role::BackStackInner,
    Role::BackIcon,
    Role::TitleWrap,
    Role::TitleInner,
    Role::Title,
    Role::Group,
    Role::GroupHeader,
    Role::Card,
    Role::Row,
    Role::RowContent,
    Role::RowIcon,
    Role::RowTitle,
    Role::RowTrailing,
    Role::RowChevron,
    Role::Separator,
    Role::SeparatorLine,
];

fn integer_property(bytes: &[u8], span: &Span, wanted: u16) -> Option<i32> {
    let mut cursor = super::cursor::Cursor::new(bytes, span.start + 5);
    let count = cursor.read16();
    for _ in 0..count {
        if cursor.failed() {
            break;
        }
        let property = cursor.read16();
        let tag = cursor.read8();
        if property == wanted && tag == 3 {
            return Some(cursor.read32() as i32);
        }
        cursor.skip_value(tag, 0);
    }
    None
}

fn remove_all_pages(out: &mut Out, state: &mut State) {
    while let Some(page) = state.pages.pop() {
        put_removal(out, &page.ids);
    }
}

fn track_navigators(state: &mut State, batch: &Batch, out: &mut Out, changed: &mut c_int, original: &[u8]) {
    let index_key = properties::current().index;
    for span in batch.spans {
        let data = batch_of(batch.bytes, span);
        if span.op == 1
            && type_is(data, span, b"navigator")
            && state.navigator(span.id).is_none()
            && state.navigators.len() < MAX_NAVIGATORS
        {
            state.navigators.push(Navigator {
                id: span.id,
                ..Navigator::default()
            });
        }
        if span.op != 2 && span.op != 3 {
            continue;
        }
        let settings = state.settings_navigator;
        let has_pages = !state.pages.is_empty();
        let Some(navigator) = state.navigator(span.id) else { continue };
        if span.op == 3 && span.child_count as usize <= MAX_NAVIGATOR_CHILDREN {
            navigator.children = (0..span.child_count).map(|position| child(data, span, position)).collect();
            if has_pages && navigator.id == settings {
                if *changed == 0 {
                    out.put(&original[..state.batch_length.min(original.len())]);
                }
                remove_all_pages(out, state);
                *changed = 1;
            }
        }
        if span.op == 2
            && let Some(value) = integer_property(data, span, index_key)
            && let Some(navigator) = state.navigator(span.id)
        {
            navigator.index = value;
        }
    }
}

#[derive(Clone, Copy)]
struct Style {
    size: f64,
    weight: i32,
    opacity: f64,
    lines: i32,
    grow: bool,
}

const fn style(size: f64, weight: i32, opacity: f64, lines: i32, grow: bool) -> Style {
    Style {
        size,
        weight,
        opacity,
        lines,
        grow,
    }
}

struct Builder<'a> {
    out: Out,
    page: &'a mut Page,
    templates: &'a [Option<Vec<u8>>],
    properties: AstralWireProperties,
}

impl Builder<'_> {
    fn allocate(&mut self) -> i32 {
        if self.page.ids.is_empty() {
            self.page.next_id = self.page.first_id;
        }
        let id = self.page.next_id;
        self.page.next_id += 1;
        if self.page.ids.len() < MAX_PAGE_NODES {
            self.page.ids.push(id);
        }
        id
    }

    fn clone_role(&mut self, role: Role, changes: &[Change]) -> i32 {
        let id = self.allocate();
        match &self.templates[role as usize] {
            Some(template) => {
                let span = Span {
                    op: 1,
                    id: 0,
                    start: 0,
                    end: template.len(),
                    children: 0,
                    child_count: 0,
                };
                clone_creation(&mut self.out, template, &span, id, changes);
            }
            None => self.out.failed = 1,
        }
        id
    }

    fn node(&mut self, kind: &[u8], properties: u16) -> i32 {
        let id = self.allocate();
        self.out.put8(1);
        self.out.put(&id.to_le_bytes());
        self.out.put16(kind.len() as u16);
        self.out.put(kind);
        self.out.prop_count(properties);
        id
    }

    fn children(&mut self, parent: i32, children: &[i32]) {
        put_children(&mut self.out, parent, children);
    }

    fn long(&mut self, property: u16, value: u32) {
        self.out.put16(property);
        self.out.put8(4);
        self.out.put(&i64::from(value).to_le_bytes());
    }

    fn flag(&mut self, property: u16, value: bool) {
        self.out.put16(property);
        self.out.put8(if value { 2 } else { 1 });
    }

    fn events(&mut self, property: u16, event: &[u8]) {
        self.out.put16(property);
        self.out.put8(7);
        self.out.put32(1);
        self.out.put8(6);
        self.out.put_string(event);
    }

    fn template_color(&self, role: Role, wanted: u16, fallback: u32) -> u32 {
        let Some(template) = &self.templates[role as usize] else {
            return fallback;
        };
        let span = Span {
            op: 1,
            id: 0,
            start: 0,
            end: template.len(),
            children: 0,
            child_count: 0,
        };
        let (mut cursor, count) = super::format::property_cursor(template, &span);
        for _ in 0..count {
            if cursor.failed() {
                break;
            }
            let property = cursor.read16();
            let tag = cursor.read8();
            if property == wanted && (tag == 3 || tag == 4) {
                return cursor.read32();
            }
            cursor.skip_value(tag, 0);
        }
        fallback
    }

    fn switch(&mut self, key: &[u8], on: bool) -> i32 {
        let p = self.properties;
        let identifier = [b"astral.switch.".as_slice(), key].concat();
        let track = self.node(b"view", 14);
        self.flag(p.accessibilityHidden, true);
        self.out.prop_string(p.accessibilityId, &identifier);
        self.out.prop_string(p.align, b"stretch");
        if on {
            self.long(p.background, SWITCH_ON);
        } else {
            self.out.prop_int(p.background, SWITCH_OFF);
        }
        self.out.prop_int(p.borderColor, SWITCH_BORDER);
        self.out.prop_number(p.borderWidth, 1.0);
        self.out.prop_string(p.direction, b"row");
        self.out.prop_number(p.height, 28.0);
        self.events(p.on, b"press");
        self.out.prop_quad(p.padding, [4.0; 4]);
        self.out.prop_string(p.pressFeedback, b"none");
        self.out.prop_number(p.radius, 16.0);
        self.out.prop_number(p.width, 56.0);
        self.flag(p.exactHitArea, true);
        let thumb = self.node(b"view", 8);
        self.out.prop_string(p.align, b"center");
        self.long(p.background, THUMB);
        self.out.prop_string(p.direction, b"row");
        self.out.prop_number(p.height, 20.0);
        self.out.prop_string(p.justify, b"center");
        self.out.prop_number(p.radius, 9999.0);
        self.out.prop_number(p.translateX, if on { 28.0 } else { 0.0 });
        self.out.prop_number(p.width, 20.0);
        self.children(track, &[thumb]);
        track
    }

    fn slider(&mut self, row: &Row) -> i32 {
        let p = self.properties;
        let steps = if row.steps > 0 { row.steps } else { 1 };
        let identifier = [format!("astral.slider.{steps}.").as_bytes(), &row.key].concat();
        let wrap = self.node(b"view", 3);
        self.out.prop_string(p.align, b"stretch");
        self.out.prop_quad(p.padding, [16.0, 0.0, 16.0, 12.0]);
        self.out.prop_string(p.width, b"fill");
        let slider = self.node(b"slider", 9);
        self.out.prop_string(p.accessibilityId, &identifier);
        self.out.prop_string(p.accessibilityLabel, &row.title);
        self.out.prop_number(p.height, 40.0);
        self.out.prop_string(p.width, b"fill");
        self.out.prop_int(p.value, row.position);
        self.out.prop_int(p.steps, steps);
        self.long(p.minTrackTint, SLIDER_ACTIVE);
        self.long(p.maxTrackTint, SLIDER_INACTIVE);
        self.events(p.on, b"change");
        self.children(wrap, &[slider]);
        wrap
    }

    fn spec_row(&mut self, row: &Row, first: bool, last: bool) -> i32 {
        let p = self.properties;
        let identifier = [b"astral.row.".as_slice(), &row.key].concat();
        let mut row_changes = vec![
            Change::text(p.accessibilityId, &identifier),
            Change::text(p.accessibilityLabel, &row.title),
            corner(p.radiusTopLeft, first),
            corner(p.radiusTopRight, first),
            corner(p.radiusBottomLeft, last),
            corner(p.radiusBottomRight, last),
        ];
        if row.kind == ROW_INFO || row.kind == ROW_NUMBER {
            row_changes.push(Change::drop(p.on));
        }
        if row.kind == ROW_SWITCH {
            row_changes.push(Change::drop(p.on));
            row_changes.push(Change::text(p.role, b"switch"));
        }
        let row_id = self.clone_role(Role::Row, &row_changes);
        let content = self.clone_role(Role::RowContent, &[]);
        let has_icon = row.icon.as_ref().is_none_or(|icon| !icon.is_empty());
        let icon_source = row.icon.clone().unwrap_or_else(|| b"res://ic_mana_sparkles".to_vec());
        let image_icon = icon_source.starts_with(b"data:");
        let mut icon_changes = vec![Change::text(p.uri, &icon_source)];
        if image_icon {
            icon_changes.push(Change::drop(p.tint));
        }
        let icon = if has_icon {
            self.clone_role(Role::RowIcon, &icon_changes)
        } else {
            0
        };
        let has_subtitle = !row.subtitle.is_empty();
        let mut title = self.clone_role(
            Role::RowTitle,
            &[
                Change::text(p.text, &row.title),
                if has_subtitle {
                    Change::drop(p.flex)
                } else {
                    Change::number(p.flex, 1.0)
                },
            ],
        );
        if has_subtitle {
            let column = self.node(b"view", 4);
            self.out.prop_string(p.direction, b"column");
            self.out.prop_number(p.flex, 1.0);
            self.out.prop_number(p.gap, 2.0);
            self.out.prop_quad(p.margin, [0.0, 0.0, 8.0, 0.0]);
            let subtitle = self.clone_role(
                Role::RowTitle,
                &[
                    Change::text(p.text, &row.subtitle),
                    Change::number(p.fontSize, 12.0),
                    Change::number(p.lineHeight, 16.0),
                    Change::number(p.opacity, 0.62),
                    Change {
                        property: p.maxLines,
                        edit: Edit::Integer(3),
                    },
                    Change::drop(p.flex),
                ],
            );
            self.children(column, &[title, subtitle]);
            title = column;
        }
        let mut children = if has_icon { vec![icon, title] } else { vec![title] };
        if !row.detail.is_empty() {
            children.push(self.clone_role(
                Role::RowTitle,
                &[
                    Change::text(p.text, &row.detail),
                    Change::number(p.fontSize, 14.0),
                    Change::number(p.opacity, 0.62),
                    Change::drop(p.flex),
                ],
            ));
        }
        match row.kind {
            ROW_LINK => {
                let trailing = self.clone_role(Role::RowTrailing, &[]);
                let chevron = self.clone_role(Role::RowChevron, &[]);
                self.children(trailing, &[chevron]);
                children.push(trailing);
            }
            ROW_SELECTED => {
                let trailing = self.clone_role(Role::RowTrailing, &[]);
                let check = self.clone_role(Role::RowChevron, &[Change::text(p.uri, b"res://ic_mana_checkmark_large_bold")]);
                self.children(trailing, &[check]);
                children.push(trailing);
            }
            ROW_SWITCH | ROW_SWITCH_LINK => {
                let trailing = self.node(b"view", 2);
                self.out.prop_string(p.align, b"stretch");
                self.out.prop_quad(p.margin, [18.0, 0.0, 0.0, 0.0]);
                let switch = self.switch(&row.key, row.on);
                self.children(trailing, &[switch]);
                children.push(trailing);
            }
            _ => {}
        }
        self.children(content, &children);
        if row.kind == ROW_NUMBER {
            let column = self.node(b"view", 2);
            self.out.prop_string(p.direction, b"column");
            self.out.prop_number(p.flex, 1.0);
            let slider = self.slider(row);
            self.children(column, &[content, slider]);
            self.children(row_id, &[column]);
            return row_id;
        }
        self.children(row_id, &[content]);
        row_id
    }

    fn colored_text(&mut self, text: &[u8], style: Style, color: u32) -> i32 {
        let Style {
            size,
            weight,
            opacity,
            lines,
            grow,
        } = style;
        let p = self.properties;
        let node = self.node(b"text", 6 + u16::from(lines > 0) + u16::from(grow));
        self.out.prop_string(p.text, text);
        self.long(p.color, color);
        self.out.prop_number(p.fontSize, size);
        self.out.prop_int(p.fontWeight, weight);
        self.out.prop_number(p.lineHeight, size + 4.0);
        self.out.prop_number(p.opacity, opacity);
        if lines > 0 {
            self.out.prop_int(p.maxLines, lines);
        }
        if grow {
            self.out.prop_number(p.flex, 1.0);
        }
        node
    }

    fn text(&mut self, text: &[u8], style: Style) -> i32 {
        let color = self.template_color(Role::RowTitle, self.properties.color, 0xFFFBFBFB);
        self.colored_text(text, style, color)
    }

    fn icon(&mut self, source: &[u8], size: f64) -> i32 {
        let p = self.properties;
        self.clone_role(
            Role::RowIcon,
            &[
                Change::text(p.uri, source),
                Change::number(p.width, size),
                Change::number(p.height, size),
                Change::drop(p.margin),
            ],
        )
    }

    fn round_button(&mut self, key: &[u8], icon: &[u8], size: f64) -> i32 {
        let p = self.properties;
        let identifier = [b"astral.row.".as_slice(), key].concat();
        let button = self.node(b"view", 10);
        self.out.prop_string(p.accessibilityId, &identifier);
        self.out.prop_string(p.align, b"center");
        self.out.prop_string(p.justify, b"center");
        self.long(p.background, TRANSLUCENT_FILL);
        self.out.prop_number(p.width, size);
        self.out.prop_number(p.height, size);
        self.out.prop_number(p.radius, size / 2.0);
        self.events(p.on, b"press");
        self.out.prop_string(p.role, b"button");
        self.out.prop_number(p.shrink, 0.0);
        let glyph = self.icon(icon, size * 0.55);
        self.children(button, &[glyph]);
        button
    }

    fn chip(&mut self, chip: &Chip) -> i32 {
        let p = self.properties;
        let identifier = [b"astral.row.".as_slice(), &chip.key].concat();
        let node = self.node(b"view", 9);
        self.out.prop_string(p.accessibilityId, &identifier);
        self.out.prop_string(p.align, b"center");
        self.out.prop_string(p.direction, b"row");
        self.long(p.background, if chip.selected { SWITCH_ON } else { TRANSLUCENT_FILL });
        self.out.prop_quad(p.padding, [14.0, 7.0, 14.0, 7.0]);
        self.out.prop_number(p.radius, 16.0);
        self.events(p.on, b"press");
        self.out.prop_string(p.role, b"button");
        self.out.prop_number(p.shrink, 0.0);
        let label = self.text(&chip.title, style(14.0, 600, 1.0, 1, false));
        self.children(node, &[label]);
        node
    }

    fn toolbar(&mut self, toolbar: &Toolbar) -> i32 {
        let p = self.properties;
        let mut children: Vec<i32> = toolbar.chips.iter().take(8).map(|chip| self.chip(chip)).collect();
        let spacer = self.node(b"view", 1);
        self.out.prop_number(p.flex, 1.0);
        children.push(spacer);
        if let Some((key, icon)) = &toolbar.button {
            children.push(self.round_button(key, icon, 36.0));
        }
        let row = self.node(b"view", 5);
        self.out.prop_string(p.direction, b"row");
        self.out.prop_string(p.align, b"center");
        self.out.prop_number(p.gap, 8.0);
        self.out.prop_string(p.width, b"fill");
        self.out.prop_quad(p.margin, [0.0, 0.0, 0.0, 4.0]);
        self.children(row, &children);
        row
    }

    fn version_tag(&mut self, text: &[u8]) -> i32 {
        let p = self.properties;
        let tag = self.node(b"view", 4);
        self.long(p.background, TRANSLUCENT_FILL);
        self.out.prop_quad(p.padding, [6.0, 2.0, 6.0, 2.0]);
        self.out.prop_number(p.radius, 6.0);
        self.out.prop_number(p.shrink, 0.0);
        let label = self.text(text, style(11.0, 600, 0.62, 1, false));
        self.children(tag, &[label]);
        tag
    }

    fn avatar(&mut self, source: &[u8], first: bool) -> i32 {
        let p = self.properties;
        let avatar = self.node(b"image", 7);
        self.out.prop_string(p.uri, source);
        self.out.prop_number(p.width, 24.0);
        self.out.prop_number(p.height, 24.0);
        self.out.prop_number(p.radius, 12.0);
        self.out.prop_string(p.scale, b"fill");
        self.out.prop_quad(p.margin, [if first { 0.0 } else { -6.0 }, 0.0, 0.0, 0.0]);
        self.out.prop_number(p.shrink, 0.0);
        avatar
    }

    fn authors(&mut self, card: &Card) -> i32 {
        let p = self.properties;
        let mut children = Vec::new();
        if !card.avatars.is_empty() {
            let avatars: Vec<i32> = card
                .avatars
                .iter()
                .enumerate()
                .map(|(index, avatar)| self.avatar(avatar, index == 0))
                .collect();
            let pile = self.node(b"view", 3);
            self.out.prop_string(p.direction, b"row");
            self.out.prop_string(p.align, b"center");
            self.out.prop_number(p.shrink, 0.0);
            self.children(pile, &avatars);
            children.push(pile);
        }
        children.push(self.text(&card.authors, style(12.0, 500, 0.62, 1, false)));
        let row = self.node(b"view", 3);
        self.out.prop_string(p.direction, b"row");
        self.out.prop_string(p.align, b"center");
        self.out.prop_number(p.gap, 6.0);
        self.children(row, &children);
        row
    }

    fn card(&mut self, card: &Card) -> i32 {
        let p = self.properties;
        let settings_key = [b"settings.".as_slice(), &card.key].concat();
        let overflow_key = [b"more.".as_slice(), &card.key].concat();
        let tile = self.node(b"view", 7);
        self.out.prop_string(p.align, b"center");
        self.out.prop_string(p.justify, b"center");
        self.long(p.background, TRANSLUCENT_FILL);
        self.out.prop_number(p.width, 40.0);
        self.out.prop_number(p.height, 40.0);
        self.out.prop_number(p.radius, 12.0);
        self.out.prop_number(p.shrink, 0.0);
        let glyph = self.icon(card.icon.as_deref().unwrap_or(b"res://astral_plug"), 22.0);
        self.children(tile, &[glyph]);
        let mut heading_children = vec![self.text(&card.title, style(16.0, 600, 1.0, 1, false))];
        if !card.authors.is_empty() {
            heading_children.push(self.authors(card));
        }
        let heading = self.node(b"view", 4);
        self.out.prop_string(p.direction, b"column");
        self.out.prop_number(p.flex, 1.0);
        self.out.prop_number(p.gap, 2.0);
        self.out.prop_number(p.minWidth, 0.0);
        self.children(heading, &heading_children);
        let mut actions_children = Vec::new();
        if card.has_settings {
            actions_children.push(self.round_button(&settings_key, b"res://ic_mana_settings", 32.0));
        }
        actions_children.push(self.round_button(&overflow_key, b"res://ic_mana_more_horizontal", 32.0));
        actions_children.push(self.switch(&card.key, card.on));
        let actions = self.node(b"view", 4);
        self.out.prop_string(p.direction, b"row");
        self.out.prop_string(p.align, b"center");
        self.out.prop_number(p.gap, 8.0);
        self.out.prop_number(p.shrink, 0.0);
        self.children(actions, &actions_children);
        let top = self.node(b"view", 4);
        self.out.prop_string(p.direction, b"row");
        self.out.prop_string(p.align, b"start");
        self.out.prop_number(p.gap, 12.0);
        self.out.prop_string(p.width, b"fill");
        self.children(top, &[tile, heading, actions]);
        let mut bottom_children = Vec::new();
        if card.description.is_empty() {
            let spacer = self.node(b"view", 1);
            self.out.prop_number(p.flex, 1.0);
            bottom_children.push(spacer);
        } else {
            bottom_children.push(self.text(&card.description, style(14.0, 400, 1.0, 0, true)));
        }
        if !card.version.is_empty() {
            bottom_children.push(self.version_tag(&card.version));
        }
        let bottom = self.node(b"view", 3);
        self.out.prop_string(p.direction, b"row");
        self.out.prop_string(p.align, b"end");
        self.out.prop_number(p.gap, 12.0);
        self.children(bottom, &bottom_children);
        let mut card_children = vec![top, bottom];
        if card.failed && !card.error.is_empty() {
            card_children.push(self.colored_text(&card.error, style(12.0, 500, 1.0, 3, false), CRITICAL_TEXT));
        }
        let background = self.template_color(Role::Row, p.background, 0x1497979F);
        let node = self.node(b"view", 9);
        self.out.prop_string(p.direction, b"column");
        self.out.prop_number(p.gap, 6.0);
        self.out.prop_quad(p.padding, [16.0; 4]);
        self.long(p.background, background);
        self.out.prop_number(p.radius, 16.0);
        self.long(p.borderColor, SUBTLE_BORDER);
        self.out.prop_number(p.borderWidth, 1.0);
        self.out.prop_number(p.opacity, if card.failed { 0.5 } else { 1.0 });
        self.out.prop_string(p.width, b"fill");
        self.children(node, &card_children);
        node
    }

    fn card_list(&mut self, page: &PageSpec) -> i32 {
        let p = self.properties;
        let mut children = Vec::new();
        if let Some(toolbar) = &page.toolbar {
            children.push(self.toolbar(toolbar));
        }
        for card in page.cards.iter().take(65 - children.len()) {
            children.push(self.card(card));
        }
        let list = self.node(b"view", 4);
        self.out.prop_string(p.direction, b"column");
        self.out.prop_number(p.gap, 12.0);
        self.out.prop_string(p.width, b"fill");
        self.out.prop_quad(p.margin, [0.0, 4.0, 0.0, 8.0]);
        self.children(list, &children);
        list
    }

    fn sections(&mut self, page: &PageSpec) -> Vec<i32> {
        let p = self.properties;
        let mut sections = Vec::new();
        if page.toolbar.is_some() || !page.cards.is_empty() {
            sections.push(self.card_list(page));
        }
        for section in &page.sections {
            if sections.len() >= 16 {
                break;
            }
            let group = self.clone_role(Role::Group, &[]);
            let header = self.clone_role(Role::GroupHeader, &[Change::text(p.text, &section.title)]);
            let card = self.clone_role(Role::Card, &[]);
            let mut card_children = Vec::new();
            for (index, row) in section.rows.iter().enumerate() {
                if card_children.len() >= 62 {
                    break;
                }
                if index > 0 {
                    let separator = self.clone_role(Role::Separator, &[]);
                    let line = self.clone_role(Role::SeparatorLine, &[]);
                    self.children(separator, &[line]);
                    card_children.push(separator);
                }
                card_children.push(self.spec_row(row, index == 0, index + 1 == section.rows.len()));
            }
            self.children(group, &[header, card]);
            self.children(card, &card_children);
            sections.push(group);
        }
        sections
    }
}

fn empty_buffer(buffer: *mut AstralWireBuffer) -> Option<&'static mut AstralWireBuffer> {
    let buffer = unsafe { buffer.as_mut() }?;
    *buffer = AstralWireBuffer {
        bytes: core::ptr::null_mut(),
        length: 0,
    };
    Some(buffer)
}

fn put_index(out: &mut Out, navigator: i32, index: i32) {
    out.update(navigator, 1);
    out.prop_int(properties::current().index, index);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_page_push(spec: *const AstralPageSpec, buffer: *mut AstralWireBuffer) -> c_int {
    let (Some(buffer), Some(spec)) = (empty_buffer(buffer), unsafe { spec.as_ref() }) else {
        return 0;
    };
    let page = page_of(spec);
    with_state(|state| {
        let Some(navigator) = state
            .navigators
            .iter()
            .find(|navigator| navigator.id == state.settings_navigator)
            .cloned()
        else {
            return 0;
        };
        if !state.templates_ready
            || state.pages.len() >= MAX_PAGES
            || navigator.children.is_empty()
            || navigator.children.len() >= MAX_NAVIGATOR_CHILDREN
        {
            return 0;
        }
        let p = properties::current();
        let mut new_page = Page {
            first_id: FIRST_PAGE_ID + state.pages.len() as i32 * PAGE_ID_STRIDE,
            ..Page::default()
        };
        let mut builder = Builder {
            out: Out::batch(),
            page: &mut new_page,
            templates: &state.templates,
            properties: p,
        };
        let root = builder.clone_role(
            Role::Root,
            &[
                Change::drop(p.presentation),
                Change {
                    property: p.fullScreenBack,
                    edit: Edit::Flag(true),
                },
                Change {
                    property: p.uikitPush,
                    edit: Edit::Flag(true),
                },
            ],
        );
        let phone = builder.clone_role(Role::Phone, &[]);
        let column = builder.clone_role(Role::Column, &[]);
        let container = builder.clone_role(Role::Container, &[]);
        let spacer = builder.clone_role(Role::Spacer, &[]);
        let hairline = builder.clone_role(Role::Hairline, &[]);
        let scroll = builder.clone_role(Role::Scroll, &[]);
        let list = builder.clone_role(Role::List, &[]);
        let content_start = builder.page.ids.len();
        let sections = builder.sections(&page);
        let content_end = builder.page.ids.len();
        let header_bar = builder.clone_role(Role::HeaderBar, &[]);
        let left = builder.clone_role(Role::HeaderLeft, &[]);
        let left_inner = builder.clone_role(Role::HeaderLeftInner, &[]);
        let left_pad = if state.header_has_pad {
            builder.clone_role(Role::HeaderLeftPad, &[])
        } else {
            0
        };
        let back = builder.clone_role(Role::Back, &[Change::text(p.accessibilityId, b"astral.back")]);
        let back_stack = builder.clone_role(Role::BackStack, &[]);
        let back_stack_inner = builder.clone_role(Role::BackStackInner, &[]);
        let back_icon = builder.clone_role(Role::BackIcon, &[Change::text(p.uri, b"res://ic_mana_arrow_large_left")]);
        let title_wrap = builder.clone_role(Role::TitleWrap, &[]);
        let title_inner = if state.title_has_inner {
            builder.clone_role(Role::TitleInner, &[])
        } else {
            0
        };
        let title = builder.clone_role(
            Role::Title,
            &[
                Change::text(p.accessibilityId, b"settings-astral-title"),
                Change::text(p.text, &page.title),
            ],
        );
        builder.children(root, &[phone]);
        builder.children(phone, &[column]);
        builder.children(column, &[container]);
        builder.children(container, &[spacer, hairline, scroll, header_bar]);
        builder.children(scroll, &[list]);
        builder.children(list, &sections);
        builder.children(header_bar, &[left, title_wrap]);
        builder.children(left, &[left_inner]);
        if left_pad != 0 {
            builder.children(left_inner, &[left_pad]);
            builder.children(left_pad, &[back]);
        } else {
            builder.children(left_inner, &[back]);
        }
        builder.children(back, &[back_stack]);
        builder.children(back_stack, &[back_stack_inner]);
        builder.children(back_stack_inner, &[back_icon]);
        if title_inner != 0 {
            builder.children(title_wrap, &[title_inner]);
            builder.children(title_inner, &[title]);
        } else {
            builder.children(title_wrap, &[title]);
        }
        let mut stack = navigator.children.clone();
        stack.push(root);
        builder.children(navigator.id, &stack);
        put_index(&mut builder.out, navigator.id, navigator.children.len() as i32);
        let out = builder.out;
        if out.failed != 0 || new_page.ids.len() >= MAX_PAGE_NODES {
            out.discard();
            return 0;
        }
        new_page.list = list;
        new_page.content_start = content_start;
        new_page.content_end = content_end;
        new_page.previous_children = navigator.children.clone();
        new_page.previous_index = navigator.index;
        if let Some(live) = state.navigator(navigator.id) {
            live.index = live.children.len() as i32;
            live.children.push(root);
        }
        state.pages.push(new_page);
        out.finish(buffer) as c_int
    })
    .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_page_refresh(spec: *const AstralPageSpec, buffer: *mut AstralWireBuffer) -> c_int {
    let (Some(buffer), Some(spec)) = (empty_buffer(buffer), unsafe { spec.as_ref() }) else {
        return 0;
    };
    let page = page_of(spec);
    with_state(|state| {
        let templates = state.templates.clone();
        let Some(current) = state.pages.last_mut() else { return 0 };
        let stale: Vec<i32> = current.ids[current.content_start..current.content_end].to_vec();
        let saved = current.ids.clone();
        let highest = saved.iter().copied().max().unwrap_or(0);
        current.ids = saved
            .iter()
            .enumerate()
            .filter(|(index, _)| *index < current.content_start || *index >= current.content_end)
            .map(|(_, id)| *id)
            .collect();
        current.next_id = highest + 1;
        let content_start = current.ids.len();
        let list = current.list;
        let mut builder = Builder {
            out: Out::batch(),
            page: current,
            templates: &templates,
            properties: properties::current(),
        };
        let sections = builder.sections(&page);
        builder.children(list, &sections);
        put_removal(&mut builder.out, &stale);
        let out = builder.out;
        if out.failed != 0 || current.ids.len() >= MAX_PAGE_NODES {
            out.discard();
            current.ids = saved;
            return 0;
        }
        current.content_start = content_start;
        current.content_end = current.ids.len();
        out.finish(buffer) as c_int
    })
    .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_page_pop(buffer: *mut AstralWireBuffer) -> c_int {
    let Some(buffer) = empty_buffer(buffer) else { return 0 };
    with_state(|state| {
        let settings = state.settings_navigator;
        if state.pages.is_empty() || state.navigator(settings).is_none() {
            return 0;
        }
        let page = state.pages.pop().unwrap_or_default();
        let mut out = Out::batch();
        put_index(&mut out, settings, page.previous_index);
        put_children(&mut out, settings, &page.previous_children);
        put_removal(&mut out, &page.ids);
        if out.failed != 0 {
            out.discard();
            state.pages.push(page);
            return 0;
        }
        if let Some(navigator) = state.navigator(settings) {
            navigator.children = page.previous_children.clone();
            navigator.index = page.previous_index;
        }
        out.finish(buffer) as c_int
    })
    .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_page_depth() -> c_int {
    with_state(|state| state.pages.len() as c_int).unwrap_or(0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wireRewritePages(
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
    let batch = Batch {
        bytes,
        spans: spans_of(pointer, count),
    };
    let original = unsafe { core::slice::from_raw_parts(bytes, length) };
    let accessibility = properties::current().accessibilityId;
    with_state(|state| {
        state.batch_length = length;
        track_navigators(state, &batch, out, changed, original);
        if state.category_active {
            let list = state.category_list;
            let removed = batch
                .spans
                .iter()
                .any(|span| span.op == 4 && (0..span.child_count).any(|position| child(batch_of(bytes, span), span, position) == list));
            if removed {
                if *changed == 0 {
                    out.put(original);
                }
                let ids = state.category_ids.clone();
                put_removal(out, &ids);
                remove_all_pages(out, state);
                state.category_active = false;
                *changed = 1;
            }
        }
        if state.category_active {
            let list = state.category_list;
            if !unsafe { wireChildList(bytes, batch.spans.as_ptr(), batch.spans.len(), list) }.is_null() {
                if *changed == 0 {
                    out.put(original);
                }
                put_list_with_astral(out, state, &batch, list);
                *changed = 1;
            }
            return;
        }
        let Some(anchor) = batch
            .spans
            .iter()
            .find(|span| span.op == 1 && find_string(batch_of(bytes, span), span, accessibility) == Some(ANCHOR))
        else {
            return;
        };
        let mut category = Out::default();
        capture_templates(state, &batch, anchor.id);
        if create_category(&mut category, state, &batch, anchor.id) && category.failed == 0 {
            if *changed == 0 {
                out.put(original);
            }
            out.put(unsafe { core::slice::from_raw_parts(category.bytes, category.length) });
            *changed = 1;
        } else {
            state.category_active = false;
        }
        category.discard();
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_page_template_status() -> *const c_char {
    let status = with_state(|state| {
        if state.templates_ready {
            "ready".to_string()
        } else {
            state.template_failure.clone()
        }
    })
    .unwrap_or_default();
    let Ok(mut stored) = STATUS.lock() else { return core::ptr::null() };
    let text = CString::new(status).unwrap_or_default();
    stored.insert(text).as_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_page_navigator_count() -> u32 {
    with_state(|state| state.navigators.len() as u32).unwrap_or(0)
}
