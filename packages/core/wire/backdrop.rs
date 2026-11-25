use core::ffi::c_int;
use std::sync::Mutex;

use super::cursor::Cursor;
use super::format::{Span, batch_of, child, property_cursor, spans_of, type_is, wireChildList, wireColorProperty, wireCreation};
use super::out::{Out, wirePropGradient};
use super::properties;

use super::theme::{wireBackgroundStops, wireThemeActive};

const DEPTH: u32 = 4;
const STOPS: usize = 8;
const TARGETS: usize = 64;
const DEFAULT_ANGLE: f64 = 180.0;
const MAX_DIM: f64 = 0.9;

struct Backdrop {
    enabled: bool,
    dim: f64,
    gradient_node: i32,
    angle: f64,
    stops: Vec<u32>,
}

static BACKDROP: Mutex<Backdrop> = Mutex::new(Backdrop {
    enabled: false,
    dim: 0.0,
    gradient_node: 0,
    angle: DEFAULT_ANGLE,
    stops: Vec::new(),
});

#[unsafe(no_mangle)]
pub extern "C" fn astral_wire_set_settings_backdrop(enabled: c_int, dim: f64) {
    if let Ok(mut backdrop) = BACKDROP.lock() {
        backdrop.enabled = enabled != 0;
        backdrop.dim = dim.clamp(0.0, MAX_DIM);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_wire_settings_backdrop_enabled() -> c_int {
    BACKDROP.lock().map_or(0, |backdrop| c_int::from(backdrop.enabled))
}

fn dimmed(color: u32, dim: f64) -> u32 {
    let keep = 1.0 - dim;
    let channel = |shift: u32| (f64::from((color >> shift) & 0xFF) * keep).round() as u32;
    0xFF000000 | channel(16) << 16 | channel(8) << 8 | channel(0)
}

fn read_gradient(bytes: &[u8], span: &Span) -> Option<(f64, Vec<u32>)> {
    let properties = properties::current();
    let (mut cursor, count) = property_cursor(bytes, span);
    for _ in 0..count {
        if cursor.failed() {
            break;
        }
        let property = cursor.read16();
        let tag = cursor.read8();
        if property != properties.backgroundGradient || tag != 8 {
            cursor.skip_value(tag, 0);
            continue;
        }
        return gradient_entries(&mut cursor, bytes, properties.angle, properties.colors);
    }
    None
}

fn gradient_entries(cursor: &mut Cursor, bytes: &[u8], angle_key: u16, colors_key: u16) -> Option<(f64, Vec<u32>)> {
    let mut angle = DEFAULT_ANGLE;
    let mut stops = Vec::new();
    let entries = cursor.read32();
    for _ in 0..entries {
        if cursor.failed() {
            break;
        }
        let key = cursor.read16();
        let tag = cursor.read8();
        if key == angle_key && tag == 5 {
            if let Some(value) = cursor.take(8) {
                angle = f64::from_le_bytes(value.try_into().ok()?);
            }
        } else if key == colors_key && tag == 7 {
            let colors = cursor.read32();
            for _ in 0..colors {
                if cursor.failed() {
                    break;
                }
                let stop_tag = cursor.read8();
                let at = cursor.offset();
                cursor.skip_value(stop_tag, 2);
                if cursor.failed() || (stop_tag != 3 && stop_tag != 4) || stops.len() >= STOPS {
                    continue;
                }
                stops.push(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?));
            }
        } else {
            cursor.skip_value(tag, 1);
        }
    }
    (!cursor.failed() && !stops.is_empty()).then_some((angle, stops))
}

fn has_property(bytes: &[u8], span: &Span, wanted: u16) -> bool {
    let (mut cursor, count) = property_cursor(bytes, span);
    for _ in 0..count {
        if cursor.failed() {
            break;
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

fn observe_app_gradient(backdrop: &mut Backdrop, bytes: *const u8, spans: &[Span]) {
    let gradient = properties::current().backgroundGradient;
    for span in spans {
        if span.op != 1 && span.op != 2 {
            continue;
        }
        if span.op == 1 && backdrop.gradient_node != 0 && span.id != backdrop.gradient_node {
            continue;
        }
        if span.op == 2 && span.id != backdrop.gradient_node {
            continue;
        }
        let data = batch_of(bytes, span);
        if span.op == 1 && !type_is(data, span, b"stack") {
            continue;
        }
        match read_gradient(data, span) {
            Some((angle, stops)) => {
                backdrop.gradient_node = span.id;
                backdrop.angle = angle;
                backdrop.stops = stops;
            }
            None if span.op == 2 && has_property(data, span, gradient) => backdrop.stops.clear(),
            None => {}
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_wire_app_gradient(colors: *mut u32, capacity: usize) -> usize {
    let Ok(backdrop) = BACKDROP.lock() else { return 0 };
    let count = backdrop.stops.len().min(capacity);
    if count > 0 && !colors.is_null() {
        unsafe { core::ptr::copy_nonoverlapping(backdrop.stops.as_ptr(), colors, count) };
    }
    count
}

fn dimmed_stops(backdrop: &Backdrop) -> Vec<u32> {
    let mut stops = backdrop.stops.clone();
    if stops.is_empty() {
        let mut theme = [0u32; STOPS];
        let count = unsafe { wireBackgroundStops(theme.as_mut_ptr(), STOPS) }.min(STOPS);
        stops = theme[..count].to_vec();
    }
    stops.iter().map(|color| dimmed(*color, backdrop.dim)).collect()
}

fn collect_matching(bytes: *const u8, spans: &[Span], id: i32, background: u32, depth: u32, targets: &mut Vec<i32>) {
    if depth > DEPTH || targets.len() >= TARGETS {
        return;
    }
    let Some(list) = (unsafe { wireChildList(bytes, spans.as_ptr(), spans.len(), id).as_ref() }) else {
        return;
    };
    let background_property = properties::current().background;
    let data = batch_of(bytes, list);
    for position in 0..list.child_count {
        if targets.len() >= TARGETS {
            break;
        }
        let id = child(data, list, position);
        let creation = unsafe { wireCreation(spans.as_ptr(), spans.len(), id) };
        let mut color = 0;
        if !creation.is_null() && unsafe { wireColorProperty(bytes, creation, background_property, &mut color) } != 0 && color == background
        {
            targets.push(id);
        }
        collect_matching(bytes, spans, id, background, depth + 1, targets);
    }
}

fn put_transparent(out: &mut Out, id: i32) {
    out.update(id, 1);
    out.put16(properties::current().background);
    out.put8(4);
    out.put(&0i64.to_le_bytes());
}

fn put_backdrop(out: &mut Out, id: i32, angle: f64, stops: &[u32]) {
    let properties = properties::current();
    out.update(id, 2);
    out.put16(properties.background);
    out.put8(4);
    out.put(&0i64.to_le_bytes());
    unsafe { wirePropGradient(out, properties.backgroundGradient, angle, stops.as_ptr(), stops.len() as u32) };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wireSettingsBackdrop(
    bytes: *const u8,
    pointer: *const Span,
    count: usize,
    out: *mut Out,
    changed: *mut c_int,
    length: usize,
) {
    let spans = spans_of(pointer, count);
    let Ok(mut backdrop) = BACKDROP.lock() else { return };
    observe_app_gradient(&mut backdrop, bytes, spans);
    if !backdrop.enabled || (backdrop.stops.is_empty() && wireThemeActive() == 0) {
        return;
    }
    let mut stops = dimmed_stops(&backdrop);
    if stops.is_empty() {
        return;
    }
    if stops.len() == 1 {
        stops.push(stops[0]);
    }
    let angle = if backdrop.stops.is_empty() { DEFAULT_ANGLE } else { backdrop.angle };
    drop(backdrop);
    let (Some(out), Some(changed)) = (unsafe { out.as_mut() }, unsafe { changed.as_mut() }) else {
        return;
    };
    let properties = properties::current();
    for span in spans {
        let data = batch_of(bytes, span);
        let mut background = 0;
        if span.op != 1
            || !type_is(data, span, b"view")
            || super::format::find_string(data, span, properties.accessibilityId) != Some(b"settings-phone")
            || unsafe { wireColorProperty(bytes, span, properties.background, &mut background) } == 0
        {
            continue;
        }
        let mut targets = Vec::new();
        collect_matching(bytes, spans, span.id, background, 1, &mut targets);
        if *changed == 0 {
            out.put(unsafe { core::slice::from_raw_parts(bytes, length) });
        }
        *changed = 1;
        put_backdrop(out, span.id, angle, &stops);
        for target in targets {
            put_transparent(out, target);
        }
    }
}
