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

#[unsafe(no_mangle)]
pub extern "C" fn astral_cutout_kind(frame: AstralCutoutFrame) -> c_int {
    if frame.width <= 0.0 || frame.height <= 0.0 {
        return -1;
    }
    if frame.y <= 0.0 {
        return KIND_NOTCH;
    }
    if frame.width > frame.height { KIND_ISLAND } else { KIND_HOLE }
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_cutout_corner_radius(kind: c_int, frame: AstralCutoutFrame) -> f64 {
    match kind {
        KIND_NOTCH => frame.height * NOTCH_CORNER_RATIO,
        KIND_HOLE => frame.width.min(frame.height) / 2.0,
        _ => frame.height / 2.0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_cutout_logo_frame(kind: c_int, frame: AstralCutoutFrame) -> AstralCutoutFrame {
    let ratio = if kind == KIND_HOLE { HOLE_LOGO_RATIO } else { LOGO_HEIGHT_RATIO };
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
