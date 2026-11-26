use super::tag::{Op, Tag};
use core::ffi::c_int;
use std::sync::Mutex;

use super::cursor::Cursor;
use super::format::{Span, batch_of, property_cursor, spans_of, type_is};
use super::out::Out;
use super::properties;

use super::theme::wire_background_stops;

const STOPS: usize = 8;
const DEFAULT_ANGLE: f64 = 180.0;

struct Backdrop {
    gradient_node: i32,
    angle: f64,
    stops: Vec<u32>,
}

static BACKDROP: Mutex<Backdrop> = Mutex::new(Backdrop {
    gradient_node: 0,
    angle: DEFAULT_ANGLE,
    stops: Vec::new(),
});

fn read_gradient(bytes: &[u8], span: &Span) -> Option<(f64, Vec<u32>)> {
    let properties = properties::current();
    let (mut cursor, count) = property_cursor(bytes, span);
    for _ in 0..count {
        if cursor.failed() {
            break;
        }
        let property = cursor.read16();
        let tag = cursor.read8();
        if property != properties.background_gradient || tag != Tag::Map {
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
        if key == angle_key && tag == Tag::Double {
            if let Some(value) = cursor.take(8) {
                angle = f64::from_le_bytes(value.try_into().ok()?);
            }
        } else if key == colors_key && tag == Tag::Array {
            let colors = cursor.read32();
            for _ in 0..colors {
                if cursor.failed() {
                    break;
                }
                let stop_tag = cursor.read8();
                let at = cursor.offset();
                cursor.skip_value(stop_tag, 2);
                if cursor.failed() || (stop_tag != Tag::Int && stop_tag != Tag::Long) || stops.len() >= STOPS {
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
    let gradient = properties::current().background_gradient;
    for span in spans {
        if span.op != Op::Create && span.op != Op::Update {
            continue;
        }
        if span.op == Op::Create && backdrop.gradient_node != 0 && span.id != backdrop.gradient_node {
            continue;
        }
        if span.op == Op::Update && span.id != backdrop.gradient_node {
            continue;
        }
        let data = batch_of(bytes, span);
        if span.op == Op::Create && !type_is(data, span, b"stack") {
            continue;
        }
        match read_gradient(data, span) {
            Some((angle, stops)) => {
                backdrop.gradient_node = span.id;
                backdrop.angle = angle;
                backdrop.stops = stops;
            }
            None if span.op == Op::Update && has_property(data, span, gradient) => backdrop.stops.clear(),
            None => {}
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_wire_app_gradient(colors: *mut u32, capacity: usize, angle: *mut f64) -> usize {
    let Ok(backdrop) = BACKDROP.lock() else { return 0 };
    let mut stops = backdrop.stops.clone();
    let mut chosen = backdrop.angle;
    drop(backdrop);
    if stops.is_empty() {
        let mut theme = [0u32; STOPS];
        let count = unsafe { wire_background_stops(theme.as_mut_ptr(), STOPS) }.min(STOPS);
        stops = theme[..count].to_vec();
        chosen = DEFAULT_ANGLE;
    }
    let count = stops.len().min(capacity);
    if count > 0 && !colors.is_null() {
        unsafe { core::ptr::copy_nonoverlapping(stops.as_ptr(), colors, count) };
    }
    if let Some(angle) = unsafe { angle.as_mut() } {
        *angle = chosen;
    }
    count
}

#[unsafe(export_name = "wireObserveAppGradient")]
pub unsafe extern "C" fn wire_observe_app_gradient(
    bytes: *const u8,
    pointer: *const Span,
    count: usize,
    _out: *mut Out,
    _changed: *mut c_int,
    _length: usize,
) {
    let spans = spans_of(pointer, count);
    if let Ok(mut backdrop) = BACKDROP.lock() {
        observe_app_gradient(&mut backdrop, bytes, spans);
    }
}
