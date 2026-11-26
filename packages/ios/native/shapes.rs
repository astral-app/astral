use astral_core::cutout::{self, AstralCutoutFrame, Kind};

use crate::text::AstralText;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct AstralRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[repr(u32)]
#[derive(Clone, Copy, Default)]
pub enum AstralCutoutKind {
    #[default]
    Missing,
    Island,
    Notch,
    Hole,
}

#[repr(C)]
#[derive(Default)]
pub struct AstralCutout {
    pub kind: AstralCutoutKind,
    pub corner_radius: f64,
    pub logo: AstralRect,
}

const fn frame(rect: AstralRect) -> AstralCutoutFrame {
    AstralCutoutFrame {
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
    }
}

const fn rect(frame: AstralCutoutFrame) -> AstralRect {
    AstralRect {
        x: frame.x,
        y: frame.y,
        width: frame.width,
        height: frame.height,
    }
}

const fn kind_of(kind: Kind) -> AstralCutoutKind {
    match kind {
        Kind::Island => AstralCutoutKind::Island,
        Kind::Notch => AstralCutoutKind::Notch,
        Kind::Hole => AstralCutoutKind::Hole,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_cutout(system: AstralRect) -> AstralCutout {
    let area = frame(system);
    let Some(kind) = cutout::kind(area) else {
        return AstralCutout::default();
    };
    AstralCutout {
        kind: kind_of(kind),
        corner_radius: cutout::corner_radius(kind, area),
        logo: rect(cutout::logo_frame(kind, area)),
    }
}

pub const ASTRAL_SEGMENT_VALUES: usize = 7;

#[repr(u32)]
#[derive(Clone, Copy)]
pub enum AstralSegmentKind {
    Move,
    Line,
    Cubic,
    Quad,
    Arc,
    Close,
}

impl AstralSegmentKind {
    fn parse(code: f64) -> Option<(AstralSegmentKind, usize)> {
        use astral_core::svg_path as path;
        let kinds = [
            (path::MOVE, AstralSegmentKind::Move, 2),
            (path::LINE, AstralSegmentKind::Line, 2),
            (path::CUBIC, AstralSegmentKind::Cubic, 6),
            (path::QUAD, AstralSegmentKind::Quad, 4),
            (path::ARC, AstralSegmentKind::Arc, ASTRAL_SEGMENT_VALUES),
            (path::CLOSE, AstralSegmentKind::Close, 0),
        ];
        kinds
            .into_iter()
            .find(|(value, _, _)| (*value - code).abs() < f64::EPSILON)
            .map(|(_, kind, needed)| (kind, needed))
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AstralPathSegment {
    pub kind: AstralSegmentKind,
    pub values: [f64; ASTRAL_SEGMENT_VALUES],
}

#[repr(C)]
pub struct AstralPath {
    pub segments: *mut AstralPathSegment,
    pub count: usize,
}

fn segments(flat: &[f64]) -> Vec<AstralPathSegment> {
    let mut parsed = Vec::new();
    let mut index = 0;
    while let Some(&code) = flat.get(index) {
        let Some((kind, needed)) = AstralSegmentKind::parse(code) else {
            break;
        };
        let Some(values) = flat.get(index + 1..index + 1 + needed) else {
            break;
        };
        let mut segment = AstralPathSegment {
            kind,
            values: [0.0; ASTRAL_SEGMENT_VALUES],
        };
        segment.values[..needed].copy_from_slice(values);
        parsed.push(segment);
        index += 1 + needed;
    }
    parsed
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_svg_path(data: AstralText) -> AstralPath {
    let mut boxed = segments(&astral_core::svg_path::parse(data.bytes())).into_boxed_slice();
    let path = AstralPath {
        segments: boxed.as_mut_ptr(),
        count: boxed.len(),
    };
    core::mem::forget(boxed);
    path
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_free_path(path: AstralPath) {
    if path.segments.is_null() {
        return;
    }
    drop(unsafe {
        Box::from_raw(core::ptr::slice_from_raw_parts_mut(
            path.segments,
            path.count,
        ))
    });
}
