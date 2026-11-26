use core::ffi::{c_int, c_uint, c_void};
use std::sync::Mutex;
use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};

use super::backdrop::wire_settings_backdrop;
use super::children::wire_expand_child_edits;
use super::code_blocks::wire_code_blocks;
use super::composer::wire_track_composer;
use super::decorate::{wire_decorate, wire_observe, wire_observe_removals};
use super::emoji::{wire_emoji_batch, wire_reaction_emoji};
use super::format::{MAX_SPANS, Span, wire_scan};
use super::image_rewrite::wire_rewrite_images;
use super::images::wire_track_images;
use super::insert::wire_insert_nodes;
use super::labels::wire_relabel;
use super::mentions::wire_decorate_mentions;
use super::message_menu::{wire_message_menu, wire_track_message_nodes};
use super::message_text::wire_message_texts;
use super::names::wire_color_names;
use super::out::{AstralWireBuffer, Out, WIRE_VERSION};
use super::pages::{wire_rewrite_pages, wire_set_template_source};
use super::patch::wire_patch_nodes;
use super::reactions::wire_reaction_avatars;
use super::theme::{wire_patch_colors, wire_theme_active, wire_track_colors};

unsafe extern "C" {
    fn free(pointer: *mut c_void);
}

type Stage = unsafe extern "C" fn(*const u8, *const Span, usize, *mut Out, *mut c_int, usize);
type Expansion = unsafe extern "C" fn(*const u8, usize, *mut Span, *mut usize, *mut usize) -> *mut u8;

const STAGES: [Stage; 12] = [
    wire_decorate,
    wire_patch_nodes,
    wire_insert_nodes,
    wire_decorate_mentions,
    wire_message_menu,
    wire_color_names,
    wire_reaction_emoji,
    wire_message_texts,
    wire_code_blocks,
    wire_reaction_avatars,
    wire_settings_backdrop,
    wire_observe_removals,
];

static UNSUPPORTED_VERSION: AtomicI32 = AtomicI32::new(0);
static REWRITE_FAILURES: AtomicU32 = AtomicU32::new(0);
static REWRITE_SPANS: Mutex<Vec<Span>> = Mutex::new(Vec::new());
static THEME_SPANS: Mutex<Vec<Span>> = Mutex::new(Vec::new());

#[unsafe(no_mangle)]
pub extern "C" fn astral_wire_rewrite_failures() -> c_uint {
    REWRITE_FAILURES.load(Ordering::Relaxed)
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_wire_unsupported_version() -> c_int {
    UNSUPPORTED_VERSION.load(Ordering::Relaxed)
}

fn with_spans<T>(spans: &Mutex<Vec<Span>>, work: impl FnOnce(&mut [Span]) -> T) -> T {
    let mut guard = spans.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if guard.len() != MAX_SPANS {
        guard.resize(MAX_SPANS, Span::default());
    }
    work(&mut guard)
}

fn empty(buffer: &mut AstralWireBuffer) {
    buffer.bytes = core::ptr::null_mut();
    buffer.length = 0;
}

fn scans(bytes: *const u8, length: usize, spans: &mut [Span]) -> Option<usize> {
    let mut count = 0;
    (unsafe { wire_scan(bytes, length, spans.as_mut_ptr(), &mut count) } == 0).then_some(count)
}

struct Owned(Vec<*mut u8>);

impl Owned {
    fn keep(&mut self, pointer: *mut u8) {
        self.0.push(pointer);
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        for pointer in self.0.drain(..) {
            unsafe { free(pointer.cast()) };
        }
    }
}

fn note_version(bytes: &[u8]) -> bool {
    if let Some(version) = bytes.first().copied().filter(|version| *version != WIRE_VERSION) {
        let _ = UNSUPPORTED_VERSION.compare_exchange(0, i32::from(version), Ordering::Relaxed, Ordering::Relaxed);
    }
    UNSUPPORTED_VERSION.load(Ordering::Relaxed) != 0
}

fn rewrite(original: &[u8], spans: &mut [Span], buffer: &mut AstralWireBuffer) -> bool {
    if note_version(original) {
        return false;
    }
    let Some(mut count) = scans(original.as_ptr(), original.len(), spans) else {
        return false;
    };
    let pointer = spans.as_mut_ptr();
    let mut owned = Owned(Vec::new());
    let mut bytes = original.as_ptr();
    let mut length = original.len();
    let mut changed: c_int = 0;
    let expansions: [Expansion; 2] = [wire_expand_child_edits, wire_rewrite_images];
    for expand in expansions {
        let mut expanded_length = 0;
        let expanded = unsafe { expand(bytes, length, pointer, &mut count, &mut expanded_length) };
        if !expanded.is_null() {
            owned.keep(expanded);
            bytes = expanded;
            length = expanded_length;
            changed = 1;
        }
    }
    let mut relabelled_length = 0;
    let relabelled = unsafe { wire_relabel(bytes, length, pointer, count, &mut relabelled_length) };
    if !relabelled.is_null() {
        owned.keep(relabelled);
        bytes = relabelled;
        length = relabelled_length;
        changed = 1;
    }
    let mut emoji_length = 0;
    let emoji = unsafe { wire_emoji_batch(bytes, length, pointer, &mut count, &mut emoji_length) };
    if !emoji.is_null() {
        owned.keep(emoji);
        bytes = emoji;
        length = emoji_length;
        changed = 1;
    }
    unsafe {
        wire_track_colors(bytes, pointer, count);
        wire_track_images(bytes, pointer, count);
    }
    let raw = bytes;
    let mut themed = Vec::new();
    if wire_theme_active() != 0 {
        themed = unsafe { core::slice::from_raw_parts(bytes, length) }.to_vec();
        if unsafe { wire_patch_colors(themed.as_mut_ptr(), pointer, count) } != 0 || changed != 0 {
            bytes = themed.as_ptr();
            changed = 1;
        }
    }
    let mut out = Out::default();
    if changed != 0 {
        out.put(unsafe { core::slice::from_raw_parts(bytes, length) });
    }
    unsafe {
        wire_observe(bytes, pointer, count);
        wire_track_message_nodes(bytes, pointer, count);
        wire_track_composer(bytes, pointer, count);
        wire_set_template_source(raw);
        wire_rewrite_pages(bytes, pointer, count, &mut out, &mut changed, length);
        wire_set_template_source(core::ptr::null());
        for stage in STAGES {
            stage(bytes, pointer, count, &mut out, &mut changed, length);
        }
    }
    drop(themed);
    drop(owned);
    if changed == 0 || out.failed != 0 {
        out.discard();
        return false;
    }
    if scans(out.bytes, out.length, spans).is_none() {
        REWRITE_FAILURES.fetch_add(1, Ordering::Relaxed);
        out.discard();
        return false;
    }
    out.finish(buffer)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_settings_rewrite(bytes: *const u8, length: usize, buffer: *mut AstralWireBuffer) -> c_int {
    let Some(buffer) = (unsafe { buffer.as_mut() }) else { return 0 };
    empty(buffer);
    let original = if bytes.is_null() || length == 0 {
        &[][..]
    } else {
        unsafe { core::slice::from_raw_parts(bytes, length) }
    };
    c_int::from(with_spans(&REWRITE_SPANS, |spans| rewrite(original, spans, buffer)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_wire_buffer_free(buffer: *mut AstralWireBuffer) {
    let Some(buffer) = (unsafe { buffer.as_mut() }) else { return };
    unsafe { free(buffer.bytes.cast()) };
    empty(buffer);
}

fn theme_page(buffer: &mut AstralWireBuffer, spans: &mut [Span]) -> c_int {
    let Some(count) = scans(buffer.bytes, buffer.length, spans) else {
        return 0;
    };
    let pointer = spans.as_ptr();
    unsafe { wire_track_colors(buffer.bytes, pointer, count) };
    let changed = c_int::from(wire_theme_active() != 0 && unsafe { wire_patch_colors(buffer.bytes, pointer, count) } != 0);
    let mut out = Out::default();
    let mut appended: c_int = 0;
    unsafe { wire_settings_backdrop(buffer.bytes, pointer, count, &mut out, &mut appended, buffer.length) };
    if appended == 0 || out.failed != 0 || scans(out.bytes, out.length, spans).is_none() {
        out.discard();
        return changed;
    }
    unsafe { free(buffer.bytes.cast()) };
    buffer.bytes = out.bytes;
    buffer.length = out.length;
    1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_wire_theme_page(buffer: *mut AstralWireBuffer) -> c_int {
    let Some(buffer) = (unsafe { buffer.as_mut() }) else { return 0 };
    if buffer.bytes.is_null() {
        return 0;
    }
    with_spans(&THEME_SPANS, |spans| theme_page(buffer, spans))
}
