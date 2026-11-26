use astral_core::render::cutout::{self as cutout, AstralCutoutFrame, Kind};

use crate::text::{AstralOwned, AstralText};

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

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_render_svg(svg: AstralText, pixels: u32) -> AstralOwned {
    astral_core::render::svg::png(svg.bytes(), pixels, pixels)
        .map_or(AstralOwned::NONE, AstralOwned::new)
}
