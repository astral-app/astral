pub fn png(svg: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    if !(1..=MAX_PIXELS).contains(&width) || !(1..=MAX_PIXELS).contains(&height) {
        return None;
    }
    let tree = resvg::usvg::Tree::from_data(svg, &resvg::usvg::Options::default()).ok()?;
    let size = tree.size();
    let (width_f, height_f) = (f32::from(u16::try_from(width).ok()?), f32::from(u16::try_from(height).ok()?));
    let scale = (width_f / size.width()).min(height_f / size.height());
    let offset_x = (width_f - size.width() * scale) / 2.0;
    let offset_y = (height_f - size.height() * scale) / 2.0;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)?;
    let transform = resvg::tiny_skia::Transform::from_row(scale, 0.0, 0.0, scale, offset_x, offset_y);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    pixmap.encode_png().ok()
}

const MAX_PIXELS: u32 = 2048;
