use core::ffi::c_int;

const NOTCH_CORNER_RATIO: f64 = 0.65;
const HOLE_LOGO_RATIO: f64 = 0.6;
const LOGO_HEIGHT_RATIO: f64 = 0.8;
const LOGO_ASPECT: f64 = 191.19 / 152.94;

pub const KIND_ISLAND: c_int = 0;
pub const KIND_NOTCH: c_int = 1;
pub const KIND_HOLE: c_int = 2;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct AstralCutoutFrame {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Island,
    Notch,
    Hole,
}

impl Kind {
    const fn code(self) -> c_int {
        match self {
            Kind::Island => KIND_ISLAND,
            Kind::Notch => KIND_NOTCH,
            Kind::Hole => KIND_HOLE,
        }
    }

    const fn from_code(code: c_int) -> Kind {
        match code {
            KIND_NOTCH => Kind::Notch,
            KIND_HOLE => Kind::Hole,
            _ => Kind::Island,
        }
    }
}

pub fn kind(frame: AstralCutoutFrame) -> Option<Kind> {
    if frame.width <= 0.0 || frame.height <= 0.0 {
        return None;
    }
    if frame.y <= 0.0 {
        return Some(Kind::Notch);
    }
    Some(if frame.width > frame.height { Kind::Island } else { Kind::Hole })
}

pub fn corner_radius(kind: Kind, frame: AstralCutoutFrame) -> f64 {
    match kind {
        Kind::Notch => frame.height * NOTCH_CORNER_RATIO,
        Kind::Hole => frame.width.min(frame.height) / 2.0,
        Kind::Island => frame.height / 2.0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_cutout_kind(frame: AstralCutoutFrame) -> c_int {
    kind(frame).map_or(-1, Kind::code)
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_cutout_corner_radius(kind: c_int, frame: AstralCutoutFrame) -> f64 {
    corner_radius(Kind::from_code(kind), frame)
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_cutout_logo_frame(kind: c_int, frame: AstralCutoutFrame) -> AstralCutoutFrame {
    logo_frame(Kind::from_code(kind), frame)
}

pub fn logo_frame(kind: Kind, frame: AstralCutoutFrame) -> AstralCutoutFrame {
    let ratio = if kind == Kind::Hole { HOLE_LOGO_RATIO } else { LOGO_HEIGHT_RATIO };
    let height = frame.width.min(frame.height) * ratio;
    let width = (height * LOGO_ASPECT).min(frame.width);
    let fitted_height = width / LOGO_ASPECT;
    AstralCutoutFrame {
        x: (frame.width - width) / 2.0,
        y: (frame.height - fitted_height) / 2.0,
        width,
        height: fitted_height,
    }
}
