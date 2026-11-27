use super::tag::{Op, Tag};
use core::ffi::c_int;
use std::collections::BTreeMap;
use std::sync::Mutex;

use super::cursor::Cursor;
use super::format::{Span, batch_of, property_cursor, spans_of, type_is};
use super::out::{AstralWireBuffer, Out};
use super::patch::read_identifier;
use super::properties;

use super::theme::wire_background_stops;

const STOPS: usize = 8;
const DEFAULT_ANGLE: f64 = 180.0;

struct Backdrop {
    gradient_node: i32,
    angle: f64,
    stops: Vec<u32>,
    image: Vec<u8>,
    opacity: f64,
    bodies: BTreeMap<i32, Body>,
}

#[derive(Default)]
struct Body {
    image_node: i32,
    children: Vec<i32>,
    shown: Vec<u8>,
}

static BACKDROP: Mutex<Backdrop> = Mutex::new(Backdrop {
    gradient_node: 0,
    angle: DEFAULT_ANGLE,
    stops: Vec::new(),
    image: Vec::new(),
    opacity: 1.0,
    bodies: BTreeMap::new(),
});

const CHAT_BODY: &[u8] = b"chat-body";
const IMAGE_PROPERTIES: u16 = 5;
const MAX_BODIES: usize = 32;

fn put_image(out: &mut Out, body: &mut Body, image: &[u8], opacity: f64) {
    let properties = properties::current();
    if body.image_node == 0 {
        body.image_node = super::decorate::wire_allocate_node();
        out.create(body.image_node, b"image", IMAGE_PROPERTIES);
    } else {
        out.update(body.image_node, IMAGE_PROPERTIES);
    }
    out.prop_string(properties.width, b"fill");
    out.prop_string(properties.height, b"fill");
    out.prop_string(properties.resize_mode, b"cover");
    out.prop_number(properties.opacity, opacity);
    out.prop_string(properties.uri, image);
    body.shown = image.to_vec();
}

fn put_body(out: &mut Out, id: i32, body: &mut Body, image: &[u8], opacity: f64) {
    if !image.is_empty() && body.shown != image {
        put_image(out, body, image, opacity);
    }
    let image_node = body.image_node;
    let mut children: Vec<i32> = body.children.iter().copied().filter(|child| *child != image_node).collect();
    if !image.is_empty() && image_node != 0 {
        children.insert(0, image_node);
    }
    out.put_op(Op::SetChildren);
    out.put(&id.to_le_bytes());
    out.put32(children.len() as u32);
    for child in children {
        out.put(&child.to_le_bytes());
    }
}

fn observe_bodies(backdrop: &mut Backdrop, bytes: *const u8, spans: &[Span]) -> Vec<i32> {
    let mut touched = Vec::new();
    for span in spans {
        let data = batch_of(bytes, span);
        match Op::parse(span.op) {
            Some(Op::Create) if read_identifier(data, span).is_some_and(|(identifier, _)| identifier == CHAT_BODY) => {
                if backdrop.bodies.len() >= MAX_BODIES {
                    continue;
                }
                backdrop.bodies.entry(span.id).or_default();
            }
            Some(Op::SetChildren) => {
                let Some(body) = backdrop.bodies.get_mut(&span.id) else { continue };
                body.children = (0..span.child_count).map(|index| super::format::child(data, span, index)).collect();
                body.shown.clear();
                touched.push(span.id);
            }
            Some(Op::Remove) => {
                for index in 0..span.child_count {
                    backdrop.bodies.remove(&super::format::child(data, span, index));
                }
            }
            _ => {}
        }
    }
    touched
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
    out: *mut Out,
    changed: *mut c_int,
    length: usize,
) {
    let spans = spans_of(pointer, count);
    let Ok(mut backdrop) = BACKDROP.lock() else { return };
    observe_app_gradient(&mut backdrop, bytes, spans);
    let touched = observe_bodies(&mut backdrop, bytes, spans);
    if touched.is_empty() || backdrop.image.is_empty() {
        return;
    }
    let (Some(out), Some(changed)) = (unsafe { out.as_mut() }, unsafe { changed.as_mut() }) else {
        return;
    };
    if *changed == 0 {
        out.put(unsafe { core::slice::from_raw_parts(bytes, length) });
    }
    *changed = 1;
    let (image, opacity) = (backdrop.image.clone(), backdrop.opacity);
    for id in touched {
        if let Some(body) = backdrop.bodies.get_mut(&id) {
            put_body(out, id, body, &image, opacity);
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_wire_set_background_image(uri: *const u8, length: usize, opacity: f64) {
    let image = if uri.is_null() {
        Vec::new()
    } else {
        unsafe { core::slice::from_raw_parts(uri, length) }.to_vec()
    };
    if let Ok(mut backdrop) = BACKDROP.lock() {
        backdrop.image = image;
        backdrop.opacity = if opacity > 0.0 { opacity.min(1.0) } else { 1.0 };
        for body in backdrop.bodies.values_mut() {
            body.shown.clear();
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_wire_refresh_backdrop(buffer: *mut AstralWireBuffer) -> c_int {
    let Some(buffer) = (unsafe { buffer.as_mut() }) else { return 0 };
    *buffer = AstralWireBuffer {
        bytes: core::ptr::null_mut(),
        length: 0,
    };
    let Ok(mut backdrop) = BACKDROP.lock() else { return 0 };
    let (image, opacity) = (backdrop.image.clone(), backdrop.opacity);
    let mut out = Out::batch();
    let mut any = false;
    for (id, body) in backdrop.bodies.iter_mut() {
        if !body.children.is_empty() && (!image.is_empty() || body.image_node != 0) {
            put_body(&mut out, *id, body, &image, opacity);
            any = true;
        }
    }
    if !any {
        out.discard();
        return 0;
    }
    c_int::from(out.finish(buffer))
}
