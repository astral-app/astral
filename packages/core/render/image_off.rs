use serde_json::Value;

use crate::lua::enums::FontWeight;

const AVERAGE_GLYPH_WIDTH: f32 = 0.55;

pub fn measure(content: &str, size: f32, _weight: FontWeight, _mono: bool) -> f32 {
    content.chars().count() as f32 * size * AVERAGE_GLYPH_WIDTH
}

pub fn render(_spec: &Value) -> Option<Vec<u8>> {
    None
}
