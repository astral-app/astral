use super::tag::Op;
use core::ffi::c_int;
use std::collections::HashMap;
use std::sync::Mutex;

use super::format::{Span, batch_of, child, find_string, spans_of, type_is};
use super::out::{AstralWireBuffer, Out};
use super::properties;

const CAPACITY: usize = 8192;
const MAX_NAME: usize = 64;
const ICON_PREFIX: &[u8] = b"res://ic_";
const SCHEME: usize = b"res://".len();

static IMAGES: Mutex<Option<HashMap<i32, Vec<u8>>>> = Mutex::new(None);

fn icon_name(uri: &[u8]) -> Option<&[u8]> {
    if uri.len() <= ICON_PREFIX.len() || !uri.starts_with(ICON_PREFIX) {
        return None;
    }
    let name = &uri[SCHEME..];
    let name = name.split(|byte| *byte == b'#').next().unwrap_or(name);
    (name.len() < MAX_NAME).then_some(name)
}

fn observe(images: &mut HashMap<i32, Vec<u8>>, bytes: &[u8], span: &Span) {
    let Some(uri) = find_string(bytes, span, properties::current().uri) else {
        return;
    };
    let Some(name) = icon_name(uri) else { return };
    if images.len() >= CAPACITY && !images.contains_key(&span.id) {
        return;
    }
    images.insert(span.id, name.to_vec());
}

#[unsafe(export_name = "wireTrackImages")]
pub unsafe extern "C" fn wire_track_images(bytes: *const u8, pointer: *const Span, count: usize) {
    let Ok(mut images) = IMAGES.lock() else { return };
    let images = images.get_or_insert_with(HashMap::new);
    for span in spans_of(pointer, count) {
        let data = batch_of(bytes, span);
        if (span.op == Op::Create && type_is(data, span, b"image")) || (span.op == Op::Update && images.contains_key(&span.id)) {
            observe(images, data, span);
        }
        if span.op != Op::Remove {
            continue;
        }
        for position in 0..span.child_count {
            if let Some(name) = images.get_mut(&child(data, span, position)) {
                name.clear();
            }
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_wire_refresh_images(buffer: *mut AstralWireBuffer, generation: u32) -> c_int {
    let Some(buffer) = (unsafe { buffer.as_mut() }) else { return 0 };
    *buffer = AstralWireBuffer {
        bytes: core::ptr::null_mut(),
        length: 0,
    };
    let Ok(images) = IMAGES.lock() else { return 0 };
    let Some(images) = images.as_ref() else { return 0 };
    let uri_property = properties::current().uri;
    let mut out = Out::batch();
    let mut updates = 0;
    for (id, name) in images.iter().filter(|(_, name)| !name.is_empty()) {
        let mut uri = b"res://".to_vec();
        uri.extend_from_slice(name);
        uri.extend_from_slice(format!("#{generation}").as_bytes());
        out.update(*id, 1);
        out.prop_string(uri_property, &uri);
        updates += 1;
    }
    if updates == 0 {
        out.discard();
        return 0;
    }
    if out.finish(buffer) { updates } else { 0 }
}
