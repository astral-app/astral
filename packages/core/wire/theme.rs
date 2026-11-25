use core::ffi::c_int;
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use super::cursor::Cursor;
use super::format::{Span, child, spans_of};
use super::out::{AstralWireBuffer, Out};
use super::properties::{self, AstralWireProperties};

const MAX_PALETTE: usize = 1024;
const MAX_BACKGROUND_STOPS: usize = 8;
const SURFACES: usize = 3;
const SURFACE_BACKGROUNDS: c_int = 0;
const SURFACE_TEXT: c_int = 1;
const SURFACE_ACCENTS: c_int = 2;
const TRACKED_LIMIT: usize = 24576;
const FIRST_ASTRAL_ID: i32 = 0x7a000000;
const DEFAULT_SATURATION: f64 = 0.45;
const DEFAULT_NEUTRAL: f64 = 0.3;
const MAX_SCALED_VALUE: f64 = 9000.0;

#[derive(Clone, Copy, Default)]
struct SurfaceTint {
    color: u32,
    saturation: f64,
}

struct Theme {
    palette: Vec<(u32, u32)>,
    radius_scale: f64,
    font_scale: f64,
    background_stops: Vec<u32>,
    tint: u32,
    tint_saturation: f64,
    tint_neutral: f64,
    surfaces: [SurfaceTint; SURFACES],
}

static THEME: Mutex<Theme> = Mutex::new(Theme {
    palette: Vec::new(),
    radius_scale: 1.0,
    font_scale: 1.0,
    background_stops: Vec::new(),
    tint: 0,
    tint_saturation: 0.0,
    tint_neutral: 0.0,
    surfaces: [SurfaceTint { color: 0, saturation: 0.0 }; SURFACES],
});

#[derive(Clone)]
enum TrackedValue {
    Color { tag: u8, color: u32 },
    Number(f64),
    Gradient(Vec<u8>),
}

static TRACKED: Mutex<Option<HashMap<(i32, u16), TrackedValue>>> = Mutex::new(None);
static DROPPED: AtomicU32 = AtomicU32::new(0);

fn theme<T>(work: impl FnOnce(&mut Theme) -> T) -> Option<T> {
    THEME.lock().ok().map(|mut theme| work(&mut theme))
}

fn slice<'a, T>(pointer: *const T, count: usize) -> &'a [T] {
    if pointer.is_null() || count == 0 {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(pointer, count) }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_wire_set_palette(from: *const u32, to: *const u32, count: usize) {
    let (from, to) = (slice(from, count), slice(to, count));
    let mut palette: Vec<(u32, u32)> = from.iter().zip(to).take(MAX_PALETTE).map(|(from, to)| (*from, *to)).collect();
    palette.sort_by_key(|entry| entry.0);
    palette.dedup_by_key(|entry| entry.0);
    theme(|theme| theme.palette = palette);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_wire_set_background_gradient(colors: *const u32, count: usize) {
    let stops = slice(colors, count.min(MAX_BACKGROUND_STOPS)).to_vec();
    theme(|theme| theme.background_stops = stops);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wireBackgroundStops(colors: *mut u32, capacity: usize) -> usize {
    theme(|theme| {
        let count = theme.background_stops.len().min(capacity);
        if count > 0 && !colors.is_null() {
            unsafe { core::ptr::copy_nonoverlapping(theme.background_stops.as_ptr(), colors, count) };
        }
        count
    })
    .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_wire_set_scales(radius: f64, font: f64) {
    theme(|theme| {
        theme.radius_scale = if radius > 0.0 { radius } else { 1.0 };
        theme.font_scale = if font > 0.0 { font } else { 1.0 };
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_wire_set_tint(color: u32, saturation: f64, neutral: f64) {
    theme(|theme| {
        theme.tint = color;
        theme.tint_saturation = if saturation > 0.0 { saturation } else { DEFAULT_SATURATION };
        theme.tint_neutral = if neutral > 0.0 { neutral } else { DEFAULT_NEUTRAL };
        theme.surfaces = [SurfaceTint::default(); SURFACES];
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_wire_set_surface_tint(surface: c_int, color: u32, saturation: f64) {
    if !(0..SURFACES as c_int).contains(&surface) {
        return;
    }
    theme(|theme| theme.surfaces[surface as usize] = SurfaceTint { color, saturation });
}

#[derive(Clone, Copy)]
struct Hsl {
    hue: f64,
    saturation: f64,
    lightness: f64,
}

fn channel(argb: u32, shift: u32) -> f64 {
    f64::from((argb >> shift) & 0xFF) / 255.0
}

fn to_hsl(argb: u32) -> Hsl {
    let (red, green, blue) = (channel(argb, 16), channel(argb, 8), channel(argb, 0));
    let high = red.max(green.max(blue));
    let low = red.min(green.min(blue));
    let chroma = high - low;
    let mut hsl = Hsl {
        hue: 0.0,
        saturation: 0.0,
        lightness: (high + low) / 2.0,
    };
    if chroma <= 0.0 {
        return hsl;
    }
    hsl.saturation = chroma / (1.0 - (2.0 * hsl.lightness - 1.0).abs());
    hsl.hue = if high == red {
        ((green - blue) / chroma) % 6.0
    } else if high == green {
        (blue - red) / chroma + 2.0
    } else {
        (red - green) / chroma + 4.0
    };
    if hsl.hue < 0.0 {
        hsl.hue += 6.0;
    }
    hsl
}

fn from_hsl(hsl: Hsl, alpha: u32) -> u32 {
    let chroma = (1.0 - (2.0 * hsl.lightness - 1.0).abs()) * hsl.saturation;
    let second = chroma * (1.0 - ((hsl.hue % 2.0) - 1.0).abs());
    let base = hsl.lightness - chroma / 2.0;
    let (red, green, blue) = match (hsl.hue as i64) % 6 {
        0 => (chroma, second, 0.0),
        1 => (second, chroma, 0.0),
        2 => (0.0, chroma, second),
        3 => (0.0, second, chroma),
        4 => (second, 0.0, chroma),
        _ => (chroma, 0.0, second),
    };
    let byte = |value: f64| ((value + base) * 255.0).round().clamp(0.0, 255.0) as u32;
    alpha << 24 | byte(red) << 16 | byte(green) << 8 | byte(blue)
}

fn recoloured_stop(original: u32, theme: u32) -> u32 {
    let mut target = to_hsl(theme);
    target.lightness = to_hsl(original).lightness;
    from_hsl(target, ((original >> 24) * (theme >> 24) + 127) / 255)
}

fn chroma_of(argb: u32) -> f64 {
    let channels = [(argb >> 16) & 0xFF, (argb >> 8) & 0xFF, argb & 0xFF];
    f64::from(channels.iter().max().unwrap_or(&0) - channels.iter().min().unwrap_or(&0)) / 255.0
}

fn surface_of(theme: &Theme, color: u32) -> c_int {
    let neutral = if theme.tint_neutral > 0.0 {
        theme.tint_neutral
    } else {
        DEFAULT_NEUTRAL
    };
    if chroma_of(color) > neutral {
        return SURFACE_ACCENTS;
    }
    if to_hsl(color).lightness < 0.5 {
        SURFACE_BACKGROUNDS
    } else {
        SURFACE_TEXT
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_wire_surface_of(color: u32) -> c_int {
    theme(|theme| surface_of(theme, color)).unwrap_or(SURFACE_TEXT)
}

fn surface_tint(theme: &Theme, color: u32) -> Option<(u32, f64)> {
    let surface = surface_of(theme, color);
    let own = theme.surfaces[surface as usize];
    if own.color != 0 {
        return Some((
            own.color,
            if own.saturation > 0.0 {
                own.saturation
            } else {
                to_hsl(own.color).saturation
            },
        ));
    }
    (theme.tint != 0 && surface != SURFACE_ACCENTS).then_some((theme.tint, theme.tint_saturation))
}

fn any_tint(theme: &Theme) -> bool {
    theme.tint != 0 || theme.surfaces.iter().any(|surface| surface.color != 0)
}

fn themed_color(theme: &Theme, color: u32) -> Option<u32> {
    if let Ok(index) = theme.palette.binary_search_by_key(&color, |entry| entry.0) {
        return Some(theme.palette[index].1);
    }
    if color >> 24 == 0 {
        return None;
    }
    let (tint, saturation) = surface_tint(theme, color)?;
    let mut target = to_hsl(tint);
    target.lightness = to_hsl(color).lightness;
    target.saturation = saturation;
    Some(from_hsl(target, color >> 24))
}

fn background_stop(theme: &Theme, index: u32, count: u32) -> u32 {
    let stops = &theme.background_stops;
    if stops.len() == 1 || count <= 1 {
        return stops[0];
    }
    stops[index as usize * (stops.len() - 1) / (count as usize - 1)]
}

struct Write {
    at: usize,
    tag: u8,
    color: u32,
}

fn gradient_writes(theme: &Theme, bytes: &[u8], start: usize, end: usize, replace_all: bool, colors_key: u16) -> Vec<Write> {
    let mut writes = Vec::new();
    let mut cursor = Cursor::new(&bytes[..end], start);
    let entries = cursor.read32();
    for _ in 0..entries {
        if cursor.failed() {
            break;
        }
        let key = cursor.read16();
        let tag = cursor.read8();
        if key != colors_key || tag != 7 {
            cursor.skip_value(tag, 1);
            continue;
        }
        let count = cursor.read32();
        for index in 0..count {
            if cursor.failed() {
                break;
            }
            let stop_tag = cursor.read8();
            let at = cursor.offset();
            cursor.skip_value(stop_tag, 2);
            if cursor.failed() || (stop_tag != 3 && stop_tag != 4) {
                continue;
            }
            let color = u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
            let mapped = if replace_all {
                Some(recoloured_stop(color, background_stop(theme, index, count)))
            } else {
                themed_color(theme, color)
            };
            if let Some(color) = mapped {
                writes.push(Write { at, tag: stop_tag, color });
            }
        }
    }
    writes
}

fn apply(bytes: &mut [u8], writes: &[Write]) {
    for write in writes {
        bytes[write.at..write.at + 4].copy_from_slice(&write.color.to_le_bytes());
        if write.tag == 4 {
            bytes[write.at + 4..write.at + 8].fill(0);
        }
    }
}

fn radius_property(properties: &AstralWireProperties, property: u16) -> bool {
    [
        properties.radius,
        properties.radiusBottomLeft,
        properties.radiusBottomRight,
        properties.radiusTopLeft,
        properties.radiusTopRight,
    ]
    .contains(&property)
}

fn font_property(properties: &AstralWireProperties, property: u16) -> bool {
    property == properties.fontSize || property == properties.lineHeight
}

fn scale_for(theme: &Theme, properties: &AstralWireProperties, property: u16) -> Option<f64> {
    if radius_property(properties, property) {
        return (theme.radius_scale != 1.0).then_some(theme.radius_scale);
    }
    if font_property(properties, property) {
        return (theme.font_scale != 1.0).then_some(theme.font_scale);
    }
    None
}

fn color_property(properties: &AstralWireProperties, property: u16) -> bool {
    [
        properties.background,
        properties.color,
        properties.borderColor,
        properties.activeBackground,
        properties.activeBorderColor,
        properties.focusBorderColor,
        properties.placeholderColor,
        properties.tint,
    ]
    .contains(&property)
}

fn span_cursor<'a>(bytes: &'a [u8], span: &Span) -> (Cursor<'a>, u16) {
    let mut cursor = Cursor::new(&bytes[..span.end.min(bytes.len())], span.start + 5);
    if span.op == 1 {
        let type_length = cursor.read16() as usize;
        cursor.skip(type_length);
    }
    let count = cursor.read16();
    (cursor, count)
}

pub fn patch_colors(bytes: &mut [u8], spans: &[Span]) -> bool {
    let properties = properties::current();
    let Some(patched) = theme(|theme| {
        let mut patched = false;
        for span in spans.iter().filter(|span| span.op == 1 || span.op == 2) {
            let mut numbers = Vec::new();
            let mut colors = Vec::new();
            let (mut cursor, count) = span_cursor(bytes, span);
            for _ in 0..count {
                if cursor.failed() {
                    break;
                }
                let property = cursor.read16();
                let tag = cursor.read8();
                let at = cursor.offset();
                cursor.skip_value(tag, 0);
                if cursor.failed() {
                    break;
                }
                if tag == 5
                    && let Some(scale) = scale_for(theme, &properties, property)
                {
                    let value = f64::from_le_bytes(bytes[at..at + 8].try_into().unwrap_or([0; 8]));
                    if value > 0.0 && value < MAX_SCALED_VALUE {
                        numbers.push((at, value * scale));
                    }
                    continue;
                }
                if property == properties.backgroundGradient && tag == 8 {
                    let replace_all = !theme.background_stops.is_empty() && span.id > 0 && span.id < FIRST_ASTRAL_ID;
                    if replace_all || !theme.palette.is_empty() || any_tint(theme) {
                        colors.extend(gradient_writes(theme, bytes, at, cursor.offset(), replace_all, properties.colors));
                    }
                    continue;
                }
                if !color_property(&properties, property) || (tag != 3 && tag != 4) {
                    continue;
                }
                let color = u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
                if let Some(mapped) = themed_color(theme, color) {
                    colors.push(Write { at, tag, color: mapped });
                }
            }
            patched |= !numbers.is_empty() || !colors.is_empty();
            for (at, value) in numbers {
                bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
            }
            apply(bytes, &colors);
        }
        patched
    }) else {
        return false;
    };
    patched
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wirePatchColors(bytes: *mut u8, pointer: *const Span, count: usize) -> c_int {
    let spans = spans_of(pointer, count);
    let Some(end) = spans.iter().map(|span| span.end).max() else {
        return 0;
    };
    if bytes.is_null() {
        return 0;
    }
    c_int::from(patch_colors(unsafe { core::slice::from_raw_parts_mut(bytes, end) }, spans))
}

fn with_tracked<T>(work: impl FnOnce(&mut HashMap<(i32, u16), TrackedValue>) -> T) -> Option<T> {
    let mut tracked = TRACKED.lock().ok()?;
    Some(work(tracked.get_or_insert_with(HashMap::new)))
}

fn tracked_property(properties: &AstralWireProperties, property: u16, tag: u8) -> bool {
    match tag {
        3 | 4 => color_property(properties, property),
        5 => radius_property(properties, property) || font_property(properties, property),
        8 => property == properties.backgroundGradient,
        _ => false,
    }
}

fn removed_properties(properties: &AstralWireProperties) -> [u16; 16] {
    [
        properties.background,
        properties.color,
        properties.borderColor,
        properties.activeBackground,
        properties.activeBorderColor,
        properties.focusBorderColor,
        properties.placeholderColor,
        properties.tint,
        properties.radius,
        properties.radiusBottomLeft,
        properties.radiusBottomRight,
        properties.radiusTopLeft,
        properties.radiusTopRight,
        properties.fontSize,
        properties.lineHeight,
        properties.backgroundGradient,
    ]
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wireTrackColors(bytes: *const u8, pointer: *const Span, count: usize) {
    let spans = spans_of(pointer, count);
    let properties = properties::current();
    with_tracked(|tracked| {
        for span in spans {
            let data = super::format::batch_of(bytes, span);
            if span.op == 4 && span.child_count > 0 {
                for position in 0..span.child_count {
                    let id = child(data, span, position);
                    for property in removed_properties(&properties) {
                        tracked.remove(&(id, property));
                    }
                }
            }
            if (span.op != 1 && span.op != 2) || span.id <= 0 || span.id >= FIRST_ASTRAL_ID {
                continue;
            }
            let (mut cursor, entries) = span_cursor(data, span);
            for _ in 0..entries {
                if cursor.failed() {
                    break;
                }
                let property = cursor.read16();
                let tag = cursor.read8();
                let at = cursor.offset();
                cursor.skip_value(tag, 0);
                if cursor.failed() || !tracked_property(&properties, property, tag) {
                    continue;
                }
                let value = &data[at..cursor.offset()];
                let entry = match tag {
                    3 | 4 => TrackedValue::Color {
                        tag,
                        color: u32::from_le_bytes([value[0], value[1], value[2], value[3]]),
                    },
                    5 => TrackedValue::Number(f64::from_le_bytes(value.try_into().unwrap_or([0; 8]))),
                    _ => TrackedValue::Gradient(value.to_vec()),
                };
                if tracked.len() >= TRACKED_LIMIT && !tracked.contains_key(&(span.id, property)) {
                    DROPPED.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
                tracked.insert((span.id, property), entry);
            }
        }
    });
}

fn put_tracked(out: &mut Out, theme: &Theme, properties: &AstralWireProperties, property: u16, value: &TrackedValue) {
    match value {
        TrackedValue::Color { tag, color } => {
            out.put8(*tag);
            out.put32(themed_color(theme, *color).unwrap_or(*color));
            if *tag == 4 {
                out.put32(0);
            }
        }
        TrackedValue::Number(number) => {
            out.put8(5);
            let scale = if font_property(properties, property) {
                theme.font_scale
            } else {
                theme.radius_scale
            };
            let value = if *number > 0.0 && *number < MAX_SCALED_VALUE {
                number * scale
            } else {
                *number
            };
            out.put(&value.to_le_bytes());
        }
        TrackedValue::Gradient(bytes) => {
            out.put8(8);
            let mut copy = bytes.clone();
            if !theme.background_stops.is_empty() || !theme.palette.is_empty() || any_tint(theme) {
                let writes = gradient_writes(theme, &copy, 0, copy.len(), !theme.background_stops.is_empty(), properties.colors);
                apply(&mut copy, &writes);
            }
            out.put(&copy);
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_wire_recolor(buffer: *mut AstralWireBuffer) -> c_int {
    let Some(buffer) = (unsafe { buffer.as_mut() }) else { return 0 };
    *buffer = AstralWireBuffer {
        bytes: core::ptr::null_mut(),
        length: 0,
    };
    let properties = properties::current();
    let Ok(theme) = THEME.lock() else { return 0 };
    let mut out = Out::batch();
    let updates = with_tracked(|tracked| {
        for ((id, property), value) in tracked.iter() {
            out.update(*id, 1);
            out.put16(*property);
            put_tracked(&mut out, &theme, &properties, *property, value);
        }
        tracked.len()
    })
    .unwrap_or(0);
    if updates == 0 {
        out.discard();
        return 0;
    }
    if out.finish(buffer) {
        updates.min(i32::MAX as usize) as c_int
    } else {
        0
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_wire_colors_dropped() -> u32 {
    DROPPED.load(Ordering::Relaxed)
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_wire_forget_colors() {
    with_tracked(HashMap::clear);
    DROPPED.store(0, Ordering::Relaxed);
}

#[unsafe(no_mangle)]
pub extern "C" fn wireThemeActive() -> c_int {
    theme(|theme| {
        c_int::from(
            !theme.palette.is_empty()
                || any_tint(theme)
                || theme.radius_scale != 1.0
                || theme.font_scale != 1.0
                || !theme.background_stops.is_empty(),
        )
    })
    .unwrap_or(0)
}
