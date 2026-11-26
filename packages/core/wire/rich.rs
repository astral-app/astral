use serde_json::Value;

use super::decorate::wire_allocate_node;
use super::out::Out;
use super::properties::{self, AstralWireProperties};
use super::tag::Tag;
use crate::lua::enums::TextStyle;

pub const MAX_NODES: usize = 4096;
const MAX_DEPTH: usize = 16;
const MAX_RUNS: usize = 512;
const EDGES: usize = 4;
const MONOSPACE: &str = "monospace";
const BOLD_WEIGHT: i32 = 600;
const BOLD: u64 = TextStyle::Bold.value() as u64;
const ITALIC: u64 = TextStyle::Italic.value() as u64;
const UNDERLINE: u64 = TextStyle::Underline.value() as u64;

struct Run {
    text: Vec<u8>,
    color: Option<i64>,
    flags: u64,
}

enum Prop {
    Text(u16, Vec<u8>),
    Number(u16, f64),
    Long(u16, i64),
    Int(u16, i32),
    Quad(u16, [f32; 4]),
    Events(u16),
    Runs(u16, Vec<Run>),
}

fn put_prop(out: &mut Out, prop: &Prop) {
    let properties = properties::current();
    match prop {
        Prop::Text(id, value) => out.prop_string(*id, value),
        Prop::Number(id, value) => out.prop_number(*id, *value),
        Prop::Long(id, value) => out.prop_long(*id, *value),
        Prop::Int(id, value) => out.prop_int(*id, *value),
        Prop::Quad(id, value) => out.prop_quad(*id, *value),
        Prop::Events(id) => out.prop_events(*id, b"press"),
        Prop::Runs(id, runs) => {
            out.put16(*id);
            out.put_tag(Tag::Array);
            out.put32(runs.len() as u32);
            for run in runs {
                let fields = 1
                    + u32::from(run.color.is_some())
                    + u32::from(run.flags & BOLD != 0)
                    + u32::from(run.flags & ITALIC != 0)
                    + u32::from(run.flags & UNDERLINE != 0);
                out.put_tag(Tag::Map);
                out.put32(fields);
                out.prop_string(properties.text, &run.text);
                if let Some(color) = run.color {
                    out.prop_long(properties.color, color);
                }
                if run.flags & BOLD != 0 {
                    out.prop_bool(properties.bold, true);
                }
                if run.flags & ITALIC != 0 {
                    out.prop_bool(properties.italic, true);
                }
                if run.flags & UNDERLINE != 0 {
                    out.prop_bool(properties.underline, true);
                }
            }
        }
    }
}

fn number(spec: &Value, key: &str) -> Option<f64> {
    spec.get(key).and_then(Value::as_f64).filter(|value| value.is_finite())
}

fn color(spec: &Value, key: &str) -> Option<i64> {
    spec.get(key).and_then(Value::as_i64)
}

fn text<'a>(spec: &'a Value, key: &str) -> Option<&'a str> {
    spec.get(key).and_then(Value::as_str)
}

fn flag(spec: &Value, key: &str) -> bool {
    spec.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn quad(spec: &Value, key: &str) -> Option<[f32; 4]> {
    match spec.get(key)? {
        Value::Number(value) => Some([value.as_f64()? as f32; 4]),
        Value::Array(values) if values.len() == EDGES => {
            let mut quad = [0f32; EDGES];
            for (slot, value) in quad.iter_mut().zip(values) {
                *slot = value.as_f64()? as f32;
            }
            Some(quad)
        }
        _ => None,
    }
}

fn runs(spec: &Value) -> Vec<Run> {
    spec.get("runs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(MAX_RUNS)
        .filter_map(|run| {
            let text = text(run, "text").filter(|text| !text.is_empty())?;
            let flags = [("bold", BOLD), ("italic", ITALIC), ("underline", UNDERLINE)]
                .iter()
                .filter(|(name, _)| flag(run, name))
                .fold(0, |flags, (_, bit)| flags | bit);
            Some(Run {
                text: text.as_bytes().to_vec(),
                color: color(run, "color"),
                flags,
            })
        })
        .collect()
}

fn radius(props: &mut Vec<Prop>, spec: &Value, properties: &AstralWireProperties) {
    match spec.get("radius") {
        Some(Value::Array(_)) => {
            let Some([top_left, top_right, bottom_right, bottom_left]) = quad(spec, "radius") else {
                return;
            };
            for (id, value) in [
                (properties.radius_top_left, top_left),
                (properties.radius_top_right, top_right),
                (properties.radius_bottom_right, bottom_right),
                (properties.radius_bottom_left, bottom_left),
            ] {
                props.push(Prop::Number(id, f64::from(value)));
            }
        }
        _ => {
            if let Some(value) = number(spec, "radius") {
                props.push(Prop::Number(properties.radius, value));
            }
        }
    }
}

fn layout(props: &mut Vec<Prop>, spec: &Value, properties: &AstralWireProperties) {
    if let Some(background) = color(spec, "background") {
        props.push(Prop::Long(properties.background, background));
    }
    radius(props, spec, properties);
    for (key, id) in [("padding", properties.padding), ("margin", properties.margin)] {
        if let Some(value) = quad(spec, key) {
            props.push(Prop::Quad(id, value));
        }
    }
    if flag(spec, "fill") {
        props.push(Prop::Text(properties.width, b"fill".to_vec()));
    } else if let Some(width) = number(spec, "width") {
        props.push(Prop::Number(properties.width, width));
    }
    if flag(spec, "fill_height") {
        props.push(Prop::Text(properties.height, b"fill".to_vec()));
    } else if let Some(height) = number(spec, "height") {
        props.push(Prop::Number(properties.height, height));
    }
    if flag(spec, "grow") {
        props.push(Prop::Number(properties.flex, 1.0));
    }
    if flag(spec, "fixed") {
        props.push(Prop::Number(properties.shrink, 0.0));
    }
    if flag(spec, "row") {
        props.push(Prop::Text(properties.direction, b"row".to_vec()));
    }
    if let Some(justify) = text(spec, "justify") {
        props.push(Prop::Text(properties.justify, justify.as_bytes().to_vec()));
    }
    let align = text(spec, "align").or_else(|| flag(spec, "row").then_some("center"));
    if let Some(align) = align {
        props.push(Prop::Text(properties.align, align.as_bytes().to_vec()));
    }
    if let Some(tap) = text(spec, "tap") {
        props.push(Prop::Text(properties.accessibility_id, tap.as_bytes().to_vec()));
        props.push(Prop::Events(properties.on));
    }
}

fn text_props(props: &mut Vec<Prop>, spec: &Value, properties: &AstralWireProperties) {
    let runs = runs(spec);
    if runs.is_empty() {
        props.push(Prop::Text(
            properties.text,
            text(spec, "text").unwrap_or_default().as_bytes().to_vec(),
        ));
    } else {
        props.push(Prop::Runs(properties.spans, runs));
    }
    if let Some(color) = color(spec, "color") {
        props.push(Prop::Long(properties.color, color));
    }
    if let Some(size) = number(spec, "size") {
        props.push(Prop::Number(properties.font_size, size));
    }
    if let Some(height) = number(spec, "line_height") {
        props.push(Prop::Number(properties.line_height, height));
    }
    if text(spec, "font") == Some(MONOSPACE) {
        props.push(Prop::Text(properties.font_family, MONOSPACE.as_bytes().to_vec()));
    }
    let weight = spec
        .get("weight")
        .and_then(Value::as_i64)
        .and_then(|weight| i32::try_from(weight).ok());
    if let Some(weight) = weight.or_else(|| flag(spec, "bold").then_some(BOLD_WEIGHT)) {
        props.push(Prop::Int(properties.font_weight, weight));
    }
    if let Some(lines) = spec
        .get("max_lines")
        .and_then(Value::as_i64)
        .and_then(|lines| i32::try_from(lines).ok())
    {
        props.push(Prop::Int(properties.max_lines, lines));
    }
}

fn image_props(props: &mut Vec<Prop>, spec: &Value, properties: &AstralWireProperties) {
    if let Some(uri) = text(spec, "icon").or_else(|| text(spec, "uri")) {
        props.push(Prop::Text(properties.uri, uri.as_bytes().to_vec()));
    }
    if let Some(tint) = color(spec, "color") {
        props.push(Prop::Long(properties.tint, tint));
    }
    let scale: &[u8] = if flag(spec, "cover") { b"fill" } else { b"fit" };
    props.push(Prop::Text(properties.scale, scale.to_vec()));
}

fn put_node(out: &mut Out, created: &mut Vec<i32>, spec: &Value, depth: usize) -> Option<i32> {
    if depth > MAX_DEPTH || created.len() >= MAX_NODES || !spec.is_object() {
        return None;
    }
    let properties = properties::current();
    let kind: &[u8] = match text(spec, "type").unwrap_or("view") {
        "text" => b"text",
        "image" => b"image",
        _ => b"view",
    };
    let mut props = Vec::new();
    match kind {
        b"text" => text_props(&mut props, spec, &properties),
        b"image" => image_props(&mut props, spec, &properties),
        _ => {}
    }
    layout(&mut props, spec, &properties);
    let node = wire_allocate_node();
    created.push(node);
    out.create(node, kind, props.len() as u16);
    for prop in &props {
        put_prop(out, prop);
    }
    let children: Vec<i32> = spec
        .get("children")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|child| put_node(out, created, child, depth + 1))
        .collect();
    if !children.is_empty() {
        out.put_children(node, &children);
    }
    Some(node)
}

pub fn put_tree(out: &mut Out, created: &mut Vec<i32>, spec: &Value) -> Option<i32> {
    match spec {
        Value::Array(nodes) if nodes.len() == 1 => put_node(out, created, &nodes[0], 0),
        Value::Array(nodes) => {
            let wrapper = serde_json::json!({ "type": "view", "fill": true, "children": nodes });
            put_node(out, created, &wrapper, 0)
        }
        _ => put_node(out, created, spec, 0),
    }
}
