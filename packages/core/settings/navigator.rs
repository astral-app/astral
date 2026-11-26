use core::ffi::{c_char, c_int, c_void};
use std::cell::Cell;
use std::ffi::CString;
use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::settings::pages as settings_pages;
use crate::support::bridge::text;
use crate::wire::identifiers::astral_wire_note_identifiers;
use crate::wire::out::AstralWireBuffer;
use crate::wire::pages::{
    AstralCardSpec, AstralChipSpec, AstralPageSpec, AstralRowSpec, AstralSectionSpec, AstralToolbarSpec, astral_page_depth,
    astral_page_pop, astral_page_push, astral_page_refresh,
};
use crate::wire::rewrite::{astral_wire_buffer_free, astral_wire_theme_page, astral_wire_unsupported_version};

const OVERVIEW_ROW: &str = "settings.row.astral";
const OVERVIEW_ROW_PREFIX: &str = "settings.row.astral.";
const BACK: &str = "astral.back";
const ROW_PREFIX: &str = "astral.row.";
const SWITCH_PREFIX: &str = "astral.switch.";
const SLIDER_PREFIX: &str = "astral.slider.";

pub type Deliver = unsafe extern "C" fn(bytes: *const u8, length: usize);
pub type Work = unsafe extern "C" fn(context: *mut c_void);
pub type WithWire = unsafe extern "C" fn(work: Work, context: *mut c_void);

#[derive(Clone, Copy, PartialEq)]
enum Delivery {
    Push,
    Refresh,
    Pop,
}

struct Navigator {
    routes: Vec<String>,
    deliver: Option<Deliver>,
    with_wire: Option<WithWire>,
}

static NAVIGATOR: Mutex<Navigator> = Mutex::new(Navigator {
    routes: Vec::new(),
    deliver: None,
    with_wire: None,
});

thread_local! {
    static INSIDE: Cell<bool> = const { Cell::new(false) };
    static PENDING_REFRESH: Cell<bool> = const { Cell::new(false) };
}

fn enter<T>(fallback: T, work: impl FnOnce(&mut Navigator) -> T) -> T {
    if INSIDE.get() {
        return fallback;
    }
    let mut navigator = NAVIGATOR.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    INSIDE.set(true);
    navigator.synchronize();
    let result = work(&mut navigator);
    while PENDING_REFRESH.replace(false) {
        navigator.refresh_top();
    }
    INSIDE.set(false);
    result
}

unsafe extern "C" fn run_work(context: *mut c_void) {
    if let Some(work) = unsafe { context.cast::<&mut dyn FnMut()>().as_mut() } {
        work();
    }
}

fn starts(identifier: &str, prefix: &str) -> bool {
    identifier.starts_with(prefix)
}

fn optional_text(object: &Value, key: &str) -> Option<CString> {
    object
        .get(key)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .and_then(|text| CString::new(text).ok())
}

fn any_text(object: &Value, key: &str) -> Option<CString> {
    object.get(key).and_then(Value::as_str).and_then(|text| CString::new(text).ok())
}

fn objects(value: Option<&Value>) -> Vec<&Value> {
    match value {
        Some(object @ Value::Object(_)) => vec![object],
        Some(Value::Array(list)) => list.iter().filter(|entry| entry.is_object()).collect(),
        _ => Vec::new(),
    }
}

fn pointer(text: &Option<CString>) -> *const c_char {
    text.as_ref().map_or(core::ptr::null(), |text| text.as_ptr())
}

fn integer(object: &Value, key: &str) -> i64 {
    object.get(key).and_then(Value::as_i64).unwrap_or(0)
}

fn boolean(object: &Value, key: &str) -> c_int {
    c_int::from(object.get(key).and_then(Value::as_bool).unwrap_or(false))
}

pub fn with_spec(model: &Value, body: impl FnOnce(&AstralPageSpec)) {
    if !model.is_object() {
        return;
    }
    let empty = CString::default();
    let mut texts: Vec<Option<CString>> = Vec::new();
    let mut keep = |text: Option<CString>| -> *const c_char {
        texts.push(text);
        pointer(texts.last().unwrap_or(&None))
    };
    let sections = objects(model.get("sections"));
    let mut rows: Vec<Vec<AstralRowSpec>> = Vec::new();
    let mut section_titles = Vec::new();
    for section in &sections {
        let mut section_rows = Vec::new();
        for entry in objects(section.get("rows")) {
            let key = keep(optional_text(entry, "key"));
            let title = keep(optional_text(entry, "title"));
            section_rows.push(AstralRowSpec {
                key: if key.is_null() { empty.as_ptr() } else { key },
                title: if title.is_null() { empty.as_ptr() } else { title },
                subtitle: keep(optional_text(entry, "subtitle")),
                detail: keep(optional_text(entry, "detail")),
                icon: keep(any_text(entry, "icon")),
                kind: integer(entry, "kind") as c_int,
                on: boolean(entry, "on"),
                steps: integer(entry, "steps") as i32,
                position: integer(entry, "position") as i32,
            });
        }
        rows.push(section_rows);
        section_titles.push(keep(Some(
            CString::new(section.get("title").and_then(Value::as_str).unwrap_or_default()).unwrap_or_default(),
        )));
    }
    let specs: Vec<AstralSectionSpec> = rows
        .iter()
        .zip(&section_titles)
        .map(|(rows, title)| AstralSectionSpec {
            title: *title,
            rows: rows.as_ptr(),
            count: rows.len(),
        })
        .collect();
    let toolbar_model = model.get("toolbar").filter(|toolbar| toolbar.is_object());
    let chips: Vec<AstralChipSpec> = objects(toolbar_model.and_then(|toolbar| toolbar.get("chips")))
        .into_iter()
        .map(|chip| {
            let key = keep(optional_text(chip, "key"));
            let title = keep(optional_text(chip, "title"));
            AstralChipSpec {
                key: if key.is_null() { empty.as_ptr() } else { key },
                title: if title.is_null() { empty.as_ptr() } else { title },
                selected: boolean(chip, "selected"),
            }
        })
        .collect();
    let toolbar = toolbar_model.map(|toolbar| AstralToolbarSpec {
        chips: chips.as_ptr(),
        chip_count: chips.len(),
        button_key: keep(optional_text(toolbar, "buttonKey")),
        button_icon: keep(optional_text(toolbar, "buttonIcon")),
    });
    let cards: Vec<AstralCardSpec> = objects(model.get("cards"))
        .into_iter()
        .map(|card| {
            let key = keep(optional_text(card, "key"));
            let title = keep(optional_text(card, "title"));
            let mut avatars = [core::ptr::null(); 3];
            let mut avatar_count = 0;
            for avatar in card
                .get("avatars")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                if avatar_count < 3 {
                    avatars[avatar_count] = keep(CString::new(avatar).ok());
                    avatar_count += 1;
                }
            }
            AstralCardSpec {
                key: if key.is_null() { empty.as_ptr() } else { key },
                title: if title.is_null() { empty.as_ptr() } else { title },
                authors: keep(optional_text(card, "authors")),
                description: keep(optional_text(card, "description")),
                version: keep(optional_text(card, "version")),
                icon: keep(optional_text(card, "icon")),
                error: keep(optional_text(card, "error")),
                avatars,
                avatar_count,
                has_settings: boolean(card, "hasSettings"),
                on: boolean(card, "on"),
                failed: boolean(card, "failed"),
            }
        })
        .collect();
    let title = CString::new(model.get("title").and_then(Value::as_str).unwrap_or_default()).unwrap_or_default();
    let spec = AstralPageSpec {
        title: title.as_ptr(),
        sections: specs.as_ptr(),
        count: specs.len(),
        toolbar: toolbar
            .as_ref()
            .filter(|_| toolbar_model.is_some_and(|toolbar| toolbar.as_object().is_some_and(|object| !object.is_empty())))
            .map_or(core::ptr::null(), |toolbar| toolbar as *const AstralToolbarSpec),
        cards: cards.as_ptr(),
        card_count: cards.len(),
    };
    body(&spec);
}

impl Navigator {
    fn with_wire(&self, mut work: impl FnMut()) {
        match self.with_wire {
            Some(with_wire) => {
                let mut work: &mut dyn FnMut() = &mut work;
                unsafe { with_wire(run_work, (&mut work as *mut &mut dyn FnMut()).cast()) };
            }
            None => work(),
        }
    }

    fn build(delivery: Delivery, route: &str, merged: &mut Vec<u8>) -> bool {
        let mut buffer = AstralWireBuffer {
            bytes: core::ptr::null_mut(),
            length: 0,
        };
        let mut made = 0;
        if delivery == Delivery::Pop {
            made = unsafe { astral_page_pop(&mut buffer) };
        } else {
            with_spec(&settings_pages::model(route), |spec| {
                made = unsafe {
                    if delivery == Delivery::Push {
                        astral_page_push(spec, &mut buffer)
                    } else {
                        astral_page_refresh(spec, &mut buffer)
                    }
                };
            });
            if made != 0 && astral_wire_unsupported_version() == 0 {
                unsafe { astral_wire_theme_page(&mut buffer) };
            }
        }
        if made != 0 && !buffer.bytes.is_null() && buffer.length > 1 {
            let bytes = unsafe { core::slice::from_raw_parts(buffer.bytes, buffer.length) };
            if merged.is_empty() {
                merged.extend_from_slice(bytes);
            } else {
                merged.extend_from_slice(&bytes[1..]);
            }
        }
        unsafe { astral_wire_buffer_free(&mut buffer) };
        made != 0
    }

    fn deliver(&self, bytes: &[u8]) {
        let Some(deliver) = self.deliver.filter(|_| !bytes.is_empty()) else {
            return;
        };
        unsafe {
            astral_wire_note_identifiers(bytes.as_ptr(), bytes.len());
            deliver(bytes.as_ptr(), bytes.len());
        }
    }

    fn run(&self, steps: &[(Delivery, String)]) {
        let mut merged = Vec::new();
        self.with_wire(|| {
            for (delivery, route) in steps {
                Self::build(*delivery, route, &mut merged);
            }
        });
        self.deliver(&merged);
    }

    fn push(&mut self, route: &str) {
        self.routes.push(route.to_string());
        let depth = self.routes.len();
        let mut merged = Vec::new();
        let mut built = false;
        self.with_wire(|| built = Self::build(Delivery::Push, route, &mut merged));
        if !built && self.routes.len() == depth {
            self.routes.pop();
        }
        self.deliver(&merged);
    }

    fn refresh_top(&self) {
        if let Some(route) = self.routes.last() {
            self.run(&[(Delivery::Refresh, route.clone())]);
        }
    }

    fn pop(&mut self) {
        if self.routes.pop().is_none() {
            return;
        }
        match self.routes.last() {
            Some(route) => self.run(&[(Delivery::Pop, String::new()), (Delivery::Refresh, route.clone())]),
            None => self.run(&[(Delivery::Pop, String::new())]),
        }
    }

    fn synchronize(&mut self) {
        let mut depth = 0;
        self.with_wire(|| depth = astral_page_depth().max(0) as usize);
        self.routes.truncate(depth);
    }

    fn act(&mut self, key: &str) {
        let Some(route) = self.routes.last().cloned() else { return };
        match settings_pages::action(&route, key, Arc::new(refresh)).as_deref() {
            Some("..") => self.pop(),
            Some(next) if !next.is_empty() => self.push(next),
            _ => self.refresh_top(),
        }
    }
}

pub fn configure(deliver: Option<Deliver>, with_wire: Option<WithWire>) {
    let mut navigator = NAVIGATOR.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    navigator.deliver = deliver;
    navigator.with_wire = with_wire;
}

pub fn handles(identifier: &str) -> bool {
    identifier == OVERVIEW_ROW
        || starts(identifier, OVERVIEW_ROW_PREFIX)
        || identifier == BACK
        || starts(identifier, ROW_PREFIX)
        || starts(identifier, SWITCH_PREFIX)
        || starts(identifier, SLIDER_PREFIX)
}

pub fn tap(identifier: &str) {
    enter((), |navigator| {
        if identifier == OVERVIEW_ROW || starts(identifier, OVERVIEW_ROW_PREFIX) {
            if navigator.routes.is_empty() {
                navigator.push(identifier.strip_prefix(OVERVIEW_ROW_PREFIX).unwrap_or_default());
            }
        } else if identifier == BACK {
            navigator.pop();
        } else if let Some(rest) = identifier.strip_prefix(SWITCH_PREFIX) {
            navigator.act(&format!("switch.{rest}"));
        } else if let Some(rest) = identifier.strip_prefix(ROW_PREFIX) {
            navigator.act(rest);
        }
    });
}

fn slider_parts(identifier: &str) -> Option<(i32, &str)> {
    let rest = identifier.strip_prefix(SLIDER_PREFIX)?;
    let (steps, key) = rest.split_once('.')?;
    if steps.is_empty() || !steps.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let steps: i32 = steps.parse().ok().filter(|steps: &i32| *steps > 0)?;
    Some((steps, key))
}

pub fn slide(identifier: &str, fraction: f64) {
    enter((), |navigator| {
        let Some((steps, key)) = slider_parts(identifier).filter(|(_, key)| !key.is_empty()) else {
            return;
        };
        let Some(route) = navigator.routes.last().cloned().filter(|_| fraction.is_finite()) else {
            return;
        };
        let position = (fraction.clamp(0.0, 1.0) * f64::from(steps)).round() as i64;
        settings_pages::action(&route, &format!("{key}={position}"), Arc::new(|| {}));
        navigator.refresh_top();
    });
}

pub fn slide_position(identifier: &str, position: f64) {
    if !position.is_finite() {
        return;
    }
    if let Some((steps, _)) = slider_parts(identifier) {
        slide(identifier, position / f64::from(steps));
    }
}

pub fn back() -> bool {
    enter(false, |navigator| {
        if navigator.routes.is_empty() {
            return false;
        }
        navigator.pop();
        true
    })
}

pub fn refresh() {
    if INSIDE.get() {
        PENDING_REFRESH.set(true);
        return;
    }
    enter((), |navigator| navigator.refresh_top());
}

pub fn open() -> bool {
    enter(false, |navigator| !navigator.routes.is_empty())
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_navigator_configure(deliver: Option<Deliver>, with_wire: Option<WithWire>) {
    configure(deliver, with_wire);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_navigator_handles(identifier: *const c_char, length: usize) -> c_int {
    c_int::from(handles(&text(identifier, length)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_navigator_tap(identifier: *const c_char, length: usize) {
    tap(&text(identifier, length));
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_navigator_slide(identifier: *const c_char, length: usize, fraction: f64) {
    slide(&text(identifier, length), fraction);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_navigator_slide_position(identifier: *const c_char, length: usize, position: f64) {
    slide_position(&text(identifier, length), position);
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_navigator_back() -> c_int {
    c_int::from(back())
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_navigator_refresh() {
    refresh();
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_navigator_open() -> c_int {
    c_int::from(open())
}

pub type SpecBody = unsafe extern "C" fn(context: *mut c_void, spec: *const AstralPageSpec);

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_settings_pages_with_spec(
    model: *const c_char,
    length: usize,
    body: Option<SpecBody>,
    context: *mut c_void,
) {
    let Some(body) = body else { return };
    let Ok(model) = serde_json::from_slice::<Value>(crate::support::bridge::bytes(model, length)) else {
        return;
    };
    with_spec(&model, |spec| unsafe { body(context, spec) });
}
