use std::sync::OnceLock;

use resvg::tiny_skia::{Color, FillRule, Paint, Path, PathBuilder, Pixmap, Rect, Stroke, Transform};
use serde_json::Value;
use ttf_parser::{Face, OutlineBuilder, Tag};

use crate::lua::enums::{FontWeight, ImageItemType, TextAlign};

const DEFAULT_SCALE: f64 = 2.0;
const DEFAULT_TEXT_SIZE: f32 = 16.0;
const DEFAULT_TEXT_WIDTH: f32 = 4096.0;
const DEFAULT_CIRCLE_RADIUS: f32 = 4.0;
const DEFAULT_BORDER_WIDTH: f32 = 1.0;
const WHITE: u32 = 0xFFFF_FFFF;
const ELLIPSIS: char = '\u{2026}';
const WEIGHT_AXIS: Tag = Tag::from_bytes(b"wght");
const OPTICAL_SIZE_AXIS: Tag = Tag::from_bytes(b"opsz");
const REGULAR_WEIGHT: f32 = 400.0;
const SEMIBOLD_WEIGHT: f32 = 600.0;
const BOLD_WEIGHT: f32 = 700.0;
const FAUX_BOLD_RATIO: f32 = 0.035;
const CUBIC_ARC: f32 = 0.552_284_8;

const SANS_FONTS: &[&str] = &[
    "/System/Library/Fonts/CoreUI/SFUI.ttf",
    "/System/Library/Fonts/Core/SFUI.ttf",
    "/System/Library/Fonts/SFNS.ttf",
    "/system/fonts/Roboto-Regular.ttf",
    "/system/fonts/RobotoStatic-Regular.ttf",
];
const MONO_FONTS: &[&str] = &[
    "/System/Library/Fonts/CoreUI/SFUIMono.ttf",
    "/System/Library/Fonts/Core/SFUIMono.ttf",
    "/System/Library/Fonts/CoreAddition/SFUIMono.ttf",
    "/System/Library/Fonts/SFNSMono.ttf",
    "/System/Library/Fonts/Core/Menlo.ttc",
    "/system/fonts/DroidSansMono.ttf",
];
const FALLBACK_FONTS: &[&str] = &[
    "/System/Library/Fonts/Core/PingFang.ttc",
    "/System/Library/Fonts/CoreUI/PingFang.ttc",
    "/System/Library/Fonts/Hiragino Sans GB.ttc",
    "/system/fonts/NotoSansCJK-Regular.ttc",
];

struct Fonts {
    sans: Option<Vec<u8>>,
    mono: Option<Vec<u8>>,
    fallback: Option<Vec<u8>>,
}

fn first_readable(paths: &[&str]) -> Option<Vec<u8>> {
    paths
        .iter()
        .filter_map(|path| std::fs::read(path).ok())
        .find(|data| Face::parse(data, 0).is_ok())
}

fn fonts() -> &'static Fonts {
    static FONTS: OnceLock<Fonts> = OnceLock::new();
    FONTS.get_or_init(|| Fonts {
        sans: first_readable(SANS_FONTS),
        mono: first_readable(MONO_FONTS),
        fallback: first_readable(FALLBACK_FONTS),
    })
}

fn color(value: Option<&Value>, fallback: u32) -> Color {
    let argb = value
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value & 0xFFFF_FFFF).ok())
        .unwrap_or(fallback);
    let [alpha, red, green, blue] = argb.to_be_bytes();
    Color::from_rgba8(red, green, blue, alpha)
}

fn number(item: &Value, key: &str, fallback: f32) -> f32 {
    item.get(key)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .map_or(fallback, |value| value as f32)
}

fn text<'a>(item: &'a Value, key: &str) -> &'a str {
    item.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn paint(fill: Color) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color(fill);
    paint.anti_alias = true;
    paint
}

fn rounded_rect(x: f32, y: f32, width: f32, height: f32, radius: f32) -> Option<Path> {
    let radius = radius.clamp(0.0, width.min(height) / 2.0);
    if radius <= 0.0 {
        return Some(PathBuilder::from_rect(Rect::from_xywh(x, y, width, height)?));
    }
    let handle = radius * (1.0 - CUBIC_ARC);
    let (right, bottom) = (x + width, y + height);
    let mut builder = PathBuilder::new();
    builder.move_to(x + radius, y);
    builder.line_to(right - radius, y);
    builder.cubic_to(right - handle, y, right, y + handle, right, y + radius);
    builder.line_to(right, bottom - radius);
    builder.cubic_to(right, bottom - handle, right - handle, bottom, right - radius, bottom);
    builder.line_to(x + radius, bottom);
    builder.cubic_to(x + handle, bottom, x, bottom - handle, x, bottom - radius);
    builder.line_to(x, y + radius);
    builder.cubic_to(x, y + handle, x + handle, y, x + radius, y);
    builder.close();
    builder.finish()
}

fn draw_rect(pixmap: &mut Pixmap, item: &Value, transform: Transform) {
    let Some(path) = rounded_rect(
        number(item, "x", 0.0),
        number(item, "y", 0.0),
        number(item, "w", 0.0),
        number(item, "h", 0.0),
        number(item, "radius", 0.0),
    ) else {
        return;
    };
    pixmap.fill_path(&path, &paint(color(item.get("color"), 0)), FillRule::Winding, transform, None);
    if item.get("border").is_some() {
        let stroke = Stroke {
            width: number(item, "border_width", DEFAULT_BORDER_WIDTH),
            ..Stroke::default()
        };
        pixmap.stroke_path(&path, &paint(color(item.get("border"), 0)), &stroke, transform, None);
    }
}

fn draw_circle(pixmap: &mut Pixmap, item: &Value, transform: Transform) {
    let radius = number(item, "r", DEFAULT_CIRCLE_RADIUS);
    let Some(path) = PathBuilder::from_circle(number(item, "x", 0.0), number(item, "y", 0.0), radius) else {
        return;
    };
    pixmap.fill_path(&path, &paint(color(item.get("color"), WHITE)), FillRule::Winding, transform, None);
}

struct Glyphs<'a> {
    builder: &'a mut PathBuilder,
    origin_x: f32,
    baseline: f32,
    scale: f32,
}

impl Glyphs<'_> {
    fn point(&self, x: f32, y: f32) -> (f32, f32) {
        (self.origin_x + x * self.scale, self.baseline - y * self.scale)
    }
}

impl OutlineBuilder for Glyphs<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.point(x, y);
        self.builder.move_to(x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.point(x, y);
        self.builder.line_to(x, y);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (x1, y1) = self.point(x1, y1);
        let (x, y) = self.point(x, y);
        self.builder.quad_to(x1, y1, x, y);
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (x1, y1) = self.point(x1, y1);
        let (x2, y2) = self.point(x2, y2);
        let (x, y) = self.point(x, y);
        self.builder.cubic_to(x1, y1, x2, y2, x, y);
    }

    fn close(&mut self) {
        self.builder.close();
    }
}

struct Styled<'a> {
    face: Face<'a>,
    synthetic_bold: bool,
}

fn has_axis(face: &Face, tag: Tag) -> bool {
    face.variation_axes().into_iter().any(|axis| axis.tag == tag)
}

fn styled(data: &[u8], weight: f32, size: f32) -> Option<Styled<'_>> {
    let mut face = Face::parse(data, 0).ok()?;
    let variable = has_axis(&face, WEIGHT_AXIS);
    if variable {
        face.set_variation(WEIGHT_AXIS, weight);
    }
    if has_axis(&face, OPTICAL_SIZE_AXIS) {
        face.set_variation(OPTICAL_SIZE_AXIS, size);
    }
    Some(Styled {
        face,
        synthetic_bold: !variable && weight > REGULAR_WEIGHT,
    })
}

struct Placed {
    font: usize,
    glyph: ttf_parser::GlyphId,
    advance: f32,
    face_scale: f32,
}

fn place(faces: &[Styled], character: char, size: f32) -> Option<Placed> {
    faces.iter().enumerate().find_map(|(font, styled)| {
        let glyph = styled.face.glyph_index(character)?;
        let face_scale = size / f32::from(styled.face.units_per_em());
        let advance = f32::from(styled.face.glyph_hor_advance(glyph).unwrap_or(0)) * face_scale;
        Some(Placed {
            font,
            glyph,
            advance,
            face_scale,
        })
    })
}

fn layout(faces: &[Styled], content: &str, size: f32, width: f32) -> Vec<Placed> {
    let placed: Vec<Placed> = content.chars().filter_map(|character| place(faces, character, size)).collect();
    let total: f32 = placed.iter().map(|glyph| glyph.advance).sum();
    if total <= width {
        return placed;
    }
    let ellipsis = place(faces, ELLIPSIS, size);
    let room = width - ellipsis.as_ref().map_or(0.0, |glyph| glyph.advance);
    let mut used = 0.0;
    let mut kept: Vec<Placed> = placed
        .into_iter()
        .take_while(|glyph| {
            used += glyph.advance;
            used <= room
        })
        .collect();
    kept.extend(ellipsis);
    kept
}

fn weight_of(item: &Value) -> f32 {
    match FontWeight::parse(text(item, "weight")) {
        Some(FontWeight::Bold) => BOLD_WEIGHT,
        Some(FontWeight::Semibold) => SEMIBOLD_WEIGHT,
        _ => REGULAR_WEIGHT,
    }
}

fn faces(mono: bool, weight: f32, size: f32) -> Vec<Styled<'static>> {
    let fonts = fonts();
    let primary = if mono {
        fonts.mono.as_ref().or(fonts.sans.as_ref())
    } else {
        fonts.sans.as_ref()
    };
    [primary, fonts.fallback.as_ref()]
        .into_iter()
        .flatten()
        .filter_map(|data| styled(data, weight, size))
        .collect()
}

pub fn measure(content: &str, size: f32, weight: FontWeight, mono: bool) -> f32 {
    let weight = match weight {
        FontWeight::Bold => BOLD_WEIGHT,
        FontWeight::Semibold => SEMIBOLD_WEIGHT,
        FontWeight::Regular => REGULAR_WEIGHT,
    };
    let faces = faces(mono, weight, size);
    content
        .chars()
        .filter_map(|character| place(&faces, character, size))
        .map(|glyph| glyph.advance)
        .sum()
}

fn draw_text(pixmap: &mut Pixmap, item: &Value, transform: Transform) {
    let weight = weight_of(item);
    let size = number(item, "size", DEFAULT_TEXT_SIZE);
    let faces = faces(item.get("mono").and_then(Value::as_bool) == Some(true), weight, size);
    let Some(main) = faces.first() else {
        return;
    };
    let width = number(item, "width", DEFAULT_TEXT_WIDTH);
    let glyphs = layout(&faces, text(item, "text"), size, width);
    let measured: f32 = glyphs.iter().map(|glyph| glyph.advance).sum();
    let offset = match TextAlign::parse(text(item, "align")) {
        Some(TextAlign::Right) => width - measured,
        Some(TextAlign::Center) => (width - measured) / 2.0,
        _ => 0.0,
    };
    let main_scale = size / f32::from(main.face.units_per_em());
    let baseline = number(item, "y", 0.0) + f32::from(main.face.ascender()) * main_scale;
    let mut builder = PathBuilder::new();
    let mut pen = number(item, "x", 0.0) + offset;
    for glyph in &glyphs {
        let mut outline = Glyphs {
            builder: &mut builder,
            origin_x: pen,
            baseline,
            scale: glyph.face_scale,
        };
        faces[glyph.font].face.outline_glyph(glyph.glyph, &mut outline);
        pen += glyph.advance;
    }
    let Some(path) = builder.finish() else {
        return;
    };
    let fill = paint(color(item.get("color"), WHITE));
    pixmap.fill_path(&path, &fill, FillRule::Winding, transform, None);
    if main.synthetic_bold {
        let stroke = Stroke {
            width: size * FAUX_BOLD_RATIO,
            ..Stroke::default()
        };
        pixmap.stroke_path(&path, &fill, &stroke, transform, None);
    }
}

pub fn render(spec: &Value) -> Option<Vec<u8>> {
    let scale = spec.get("scale").and_then(Value::as_f64).unwrap_or(DEFAULT_SCALE);
    let width = spec.get("width").and_then(Value::as_f64)?;
    let height = spec.get("height").and_then(Value::as_f64)?;
    let mut pixmap = Pixmap::new((width * scale).ceil() as u32, (height * scale).ceil() as u32)?;
    if spec.get("background").is_some() {
        pixmap.fill(color(spec.get("background"), 0));
    }
    let transform = Transform::from_scale(scale as f32, scale as f32);
    for item in spec.get("items").and_then(Value::as_array).into_iter().flatten() {
        match ImageItemType::parse(text(item, "type")) {
            Some(ImageItemType::Rect) => draw_rect(&mut pixmap, item, transform),
            Some(ImageItemType::Circle) => draw_circle(&mut pixmap, item, transform),
            Some(ImageItemType::Text) => draw_text(&mut pixmap, item, transform),
            None => {}
        }
    }
    pixmap.encode_png().ok()
}
