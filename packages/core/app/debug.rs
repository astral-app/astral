use std::io::Write;
use std::sync::OnceLock;

use regex_lite::Regex;
use serde_json::{Map, Value, json};

use super::host::{self, Request};
use super::reports;
use super::state::{Platform, app};
use crate::lua::enums::{FontWeight, ImageItemType, OptionType, TextAlign};
use crate::lua::host::http::{Attachment, CommandContext, CommandReply, NativeCommand, register_native};

const WIDTH: f64 = 432.0;
const SCALE: f64 = 3.0;
const PADDING: f64 = 8.0;
const GAP: f64 = 4.0;
const COLUMNS: usize = 3;
const HEADER_HEIGHT: f64 = 30.0;
const TITLE_SIZE: f64 = 15.0;
const META_SIZE: f64 = 10.0;
const META_BASELINE: f64 = 0.9;
const LINE_BOX: f64 = 1.2;
const TILE_HEIGHT: f64 = 22.0;
const TILE_PADDING: f64 = 8.0;
const TILE_RADIUS: f64 = 7.0;
const CARD_RADIUS: f64 = 12.0;
const LABEL_SIZE: f64 = 10.0;
const VALUE_SIZE: f64 = 11.0;
const LABEL_GAP: f64 = 8.0;
const WRAP_LINE: f64 = 14.0;
const MAX_TILE_LINES: usize = 4;
const PILL_HEIGHT: f64 = 18.0;
const PILL_PADDING: f64 = 8.0;
const MAX_ALT_TEXT: usize = 1024;
const SHORT_PHOENIX: usize = 8;
const ALT_DROP_ORDER: [&str; 9] = [
    "plugins_failed",
    "markers",
    "https",
    "binary",
    "signer",
    "install",
    "source",
    "framework",
    "jailbreak",
];
const CHANNELS: [(&str, &str); 5] = [
    ("canary", "alpha"),
    ("alpha", "alpha"),
    ("ptb", "stable"),
    ("stable", "stable"),
    ("beta", "beta"),
];
const SEPARATOR: &str = " · ";
const READY: &str = "ready";
const CARD_NAME: &str = "astral-debug.png";
const PNG: &str = "image/png";
const ZIP: &str = "application/zip";
const LOG_LIMIT: usize = 512 << 10;
const STATE_LIMIT: u64 = 256 << 10;
const PRIVATE_STATE: [&str; 3] = ["settings.json", "plugin-storage.sqlite", "dev-pairing-code"];

const CARD: u32 = 0xFF0F_1115;
const TILE: u32 = 0xFF17_1A21;
const LABEL: u32 = 0xFF6B_7280;
const VALUE: u32 = 0xFFF3_F4F6;
const DIM: u32 = 0xFF9C_A3AF;
const COMMIT: u32 = 0xFFA7_8BFA;
const GOOD: u32 = 0xFF34_D399;
const GOOD_BACKGROUND: u32 = 0xFF11_231B;
const WARN: u32 = 0xFFFB_BF24;
const BAD: u32 = 0xFFF8_7171;
const BAD_BACKGROUND: u32 = 0xFF2A_1215;

#[derive(Clone)]
struct Span {
    text: String,
    color: u32,
}

fn span(text: impl Into<String>, color: u32) -> Span {
    Span { text: text.into(), color }
}

struct Patterns {
    binary_noise: Regex,
    marker_noise: Regex,
    framework_suffix: Regex,
    discord: Regex,
    named_discord: Regex,
}

fn patterns() -> &'static Patterns {
    static PATTERNS: OnceLock<Patterns> = OnceLock::new();
    PATTERNS.get_or_init(|| Patterns {
        binary_noise: Regex::new(r"^(\d+ dylibs|\d+ injected|UUID \S+)$").expect("binary noise"),
        marker_noise: Regex::new(r"^\d+ Hz$").expect("marker noise"),
        framework_suffix: Regex::new(r"(\d[\w.]*)-\d+$").expect("framework suffix"),
        discord: Regex::new(r"^(\S+) \((\S+)\)$").expect("discord"),
        named_discord: Regex::new(r"^(\S+) - ([A-Za-z]+) · (\d+)$").expect("named discord"),
    })
}

fn text<'a>(environment: &'a Value, key: &str) -> &'a str {
    environment.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn present(environment: &Value, key: &str) -> Option<String> {
    Some(text(environment, key)).filter(|value| !value.is_empty()).map(str::to_owned)
}

fn binary_text(binary: &str) -> String {
    binary
        .split(SEPARATOR)
        .filter(|part| !patterns().binary_noise.is_match(part))
        .collect::<Vec<_>>()
        .join(SEPARATOR)
}

fn kept_markers(environment: &Value) -> String {
    environment
        .get("markers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|marker| !patterns().marker_noise.is_match(marker))
        .collect::<Vec<_>>()
        .join(SEPARATOR)
}

struct DiscordParts {
    version: String,
    build: Option<String>,
    channel: Option<String>,
}

fn discord_parts(environment: &Value) -> DiscordParts {
    let full = present(environment, "discord").unwrap_or_else(|| "?".to_owned());
    let (mut version, mut build, mut named) = (None, None, None);
    if let Some(found) = patterns().discord.captures(&full) {
        version = Some(found[1].to_owned());
        build = Some(found[2].to_owned());
    } else if let Some(found) = patterns().named_discord.captures(&full) {
        version = Some(found[1].to_owned());
        named = Some(found[2].to_owned());
        build = Some(found[3].to_owned());
    }
    let raw = present(environment, "discord_channel").or(named).unwrap_or_default().to_lowercase();
    let channel = CHANNELS
        .iter()
        .find(|(name, _)| *name == raw)
        .map(|(_, channel)| (*channel).to_owned())
        .or_else(|| (!raw.is_empty()).then_some(raw));
    DiscordParts {
        version: version.unwrap_or(full),
        build,
        channel,
    }
}

struct PluginCounts {
    enabled: usize,
    installed: usize,
    failed: Vec<String>,
}

fn plugin_counts(environment: &Value) -> PluginCounts {
    let plugins: Vec<&Value> = environment
        .get("plugin_list")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .collect();
    PluginCounts {
        enabled: plugins.iter().filter(|plugin| plugin["enabled"] == json!(true)).count(),
        installed: plugins.len(),
        failed: plugins
            .iter()
            .filter(|plugin| plugin["failed"] == json!(true))
            .map(|plugin| {
                plugin["name"]
                    .as_str()
                    .or_else(|| plugin["id"].as_str())
                    .unwrap_or_default()
                    .to_owned()
            })
            .collect(),
    }
}

fn framework_version(framework: &str) -> String {
    patterns().framework_suffix.replace(framework, "$1").into_owned()
}

fn https_spans(https: &Value) -> Vec<Span> {
    if !https.is_object() {
        return Vec::new();
    }
    let issuer = https.get("issuer").and_then(Value::as_str).filter(|issuer| !issuer.is_empty());
    if https.get("intercepted").and_then(Value::as_bool) == Some(true) {
        return vec![span(format!("MITM · {}", issuer.unwrap_or("unknown CA")), WARN)];
    }
    if let Some(failure) = https.get("failure").and_then(Value::as_str).filter(|failure| !failure.is_empty()) {
        return vec![span(format!("unchecked · {failure}"), DIM)];
    }
    vec![
        span("direct", VALUE),
        span(issuer.map(|issuer| format!(" · {issuer}")).unwrap_or_default(), DIM),
    ]
}

fn system_spans(environment: &Value) -> Vec<Span> {
    let mut system = present(environment, "system").unwrap_or_else(|| "?".to_owned());
    if let Some(build) = present(environment, "os_build").filter(|_| !system.contains('(')) {
        system = format!("{system} ({build})");
    }
    let mut spans = vec![span(system, VALUE)];
    if present(environment, "os_beta").is_some() {
        spans.push(span(" · beta", WARN));
    }
    spans
}

fn crash_spans(environment: &Value) -> Vec<Span> {
    let recent = environment.get("crashes_24h").and_then(Value::as_i64).unwrap_or(0);
    let last = present(environment, "last_report");
    if recent > 0 {
        let mut spans = vec![span(format!("{recent} in 24h"), BAD)];
        if let Some(last) = last {
            spans.push(span(format!(" · {last}"), DIM));
        }
        return spans;
    }
    last.map(|last| vec![span(last, WARN)]).unwrap_or_default()
}

fn rows(environment: &Value) -> Vec<(&'static str, Vec<Span>)> {
    let discord = discord_parts(environment);
    let mut rows = Vec::new();
    let mut add = |label: &'static str, spans: Vec<Span>| {
        if !spans.is_empty() {
            rows.push((label, spans));
        }
    };
    let mut injection = vec![span(present(environment, "image").unwrap_or_else(|| "astral".to_owned()), VALUE)];
    if let Some(framework) = present(environment, "framework") {
        injection.push(span(format!(" · {}", framework_version(&framework)), DIM));
    }
    add("injection", injection);
    add(
        "jailbreak",
        present(environment, "jailbreak")
            .map(|jailbreak| vec![span(jailbreak.to_lowercase(), VALUE)])
            .unwrap_or_default(),
    );
    let details: Vec<String> = [discord.channel.clone(), discord.build.clone()].into_iter().flatten().collect();
    let mut discord_row = vec![
        span(discord.version.clone(), VALUE),
        span(format!(" {}", details.join(SEPARATOR)), DIM),
    ];
    match present(environment, "compatibility") {
        Some(compatibility) if compatibility == READY => discord_row.extend([span(SEPARATOR, DIM), span("supported", GOOD)]),
        Some(compatibility) => discord_row.extend([span(SEPARATOR, DIM), span(compatibility, WARN)]),
        None => {}
    }
    add("discord", discord_row);
    add(
        "source",
        present(environment, "source")
            .map(|source| vec![span(source, VALUE)])
            .unwrap_or_default(),
    );
    let device = present(environment, "device_name")
        .or_else(|| present(environment, "device"))
        .unwrap_or_else(|| "?".to_owned());
    add("device", vec![span(device, VALUE)]);
    add("system", system_spans(environment));
    let counts = plugin_counts(environment);
    add("plugins", vec![span(format!("{}/{}", counts.enabled, counts.installed), VALUE)]);
    add(
        "install",
        present(environment, "install")
            .map(|install| vec![span(install.replace('`', ""), VALUE)])
            .unwrap_or_default(),
    );
    add(
        "signer",
        present(environment, "signer")
            .map(|signer| vec![span(signer, VALUE)])
            .unwrap_or_default(),
    );
    add(
        "binary",
        present(environment, "binary")
            .map(|binary| vec![span(binary_text(&binary), VALUE)])
            .unwrap_or_default(),
    );
    let markers = kept_markers(environment);
    add(
        "markers",
        if markers.is_empty() {
            Vec::new()
        } else {
            vec![span(markers, VALUE)]
        },
    );
    add("https", https_spans(environment.get("https").unwrap_or(&Value::Null)));
    if let Some(phoenix) = present(environment, "phoenix") {
        let mut spans = vec![span(phoenix.chars().take(SHORT_PHOENIX).collect::<String>(), VALUE)];
        if let Some(engine) = present(environment, "engine").filter(|engine| engine != "Phoenix") {
            spans.push(span(format!(" · {engine}"), DIM));
        }
        add("phoenix", spans);
    }
    let failed = plugin_counts(environment).failed;
    add(
        "failing",
        if failed.is_empty() {
            Vec::new()
        } else {
            vec![span(failed.join(SEPARATOR), BAD)]
        },
    );
    add("crashes", crash_spans(environment));
    rows
}

fn measure(text: &str, size: f64, weight: FontWeight) -> f64 {
    f64::from(crate::render::image::measure(text, size as f32, weight, false))
}

fn spans_width(spans: &[Span]) -> f64 {
    spans.iter().map(|span| measure(&span.text, VALUE_SIZE, FontWeight::Regular)).sum()
}

fn push_piece(line: &mut Vec<Span>, piece: &str, color: u32) {
    match line.last_mut() {
        Some(last) if last.color == color => last.text.push_str(piece),
        _ => line.push(span(piece, color)),
    }
}

fn split_long(piece: &str, room: f64) -> Vec<String> {
    let mut parts = vec![String::new()];
    for character in piece.chars() {
        let current = parts.last_mut().expect("one part");
        current.push(character);
        if measure(current, VALUE_SIZE, FontWeight::Regular) > room && current.chars().count() > 1 {
            current.pop();
            parts.push(character.to_string());
        }
    }
    parts
}

fn wrap(spans: &[Span], room: f64) -> Vec<Vec<Span>> {
    let mut lines: Vec<Vec<Span>> = vec![Vec::new()];
    let mut used = 0.0;
    for item in spans {
        for word in item.text.split_inclusive(' ') {
            for piece in split_long(word, room) {
                let width = measure(&piece, VALUE_SIZE, FontWeight::Regular);
                let line = lines.last_mut().expect("one line");
                if used + width > room && !line.is_empty() {
                    lines.push(Vec::new());
                    used = 0.0;
                }
                let piece = if used == 0.0 { piece.trim_start() } else { piece.as_str() };
                push_piece(lines.last_mut().expect("one line"), piece, item.color);
                used += measure(piece, VALUE_SIZE, FontWeight::Regular);
            }
        }
    }
    lines.retain(|line| !line.is_empty());
    lines
}

struct Tile {
    label: &'static str,
    label_width: f64,
    lines: Vec<Vec<Span>>,
    columns: usize,
}

fn column_width() -> f64 {
    (WIDTH - PADDING * 2.0 - GAP * (COLUMNS - 1) as f64) / COLUMNS as f64
}

fn columns_width(columns: usize) -> f64 {
    column_width() * columns as f64 + GAP * columns.saturating_sub(1) as f64
}

fn tile(label: &'static str, spans: Vec<Span>) -> Tile {
    let label_width = measure(label, LABEL_SIZE, FontWeight::Regular);
    let needed = TILE_PADDING * 2.0 + label_width + LABEL_GAP + spans_width(&spans);
    if let Some(columns) = (1..=COLUMNS).find(|columns| columns_width(*columns) >= needed) {
        return Tile {
            label,
            label_width,
            lines: vec![spans],
            columns,
        };
    }
    let room = columns_width(COLUMNS) - TILE_PADDING * 2.0 - label_width - LABEL_GAP;
    let mut lines = wrap(&spans, room);
    lines.truncate(MAX_TILE_LINES);
    Tile {
        label,
        label_width,
        lines,
        columns: COLUMNS,
    }
}

fn tile_height(tile: &Tile) -> f64 {
    TILE_HEIGHT + tile.lines.len().saturating_sub(1) as f64 * WRAP_LINE
}

fn packed(tiles: Vec<Tile>) -> Vec<Vec<Tile>> {
    let mut rows: Vec<Vec<Tile>> = Vec::new();
    for tile in tiles {
        let taken = |row: &Vec<Tile>| row.iter().map(|tile| tile.columns).sum::<usize>();
        match rows.iter_mut().find(|row| taken(row) + tile.columns <= COLUMNS) {
            Some(row) => row.push(tile),
            None => rows.push(vec![tile]),
        }
    }
    for row in &mut rows {
        let taken: usize = row.iter().map(|tile| tile.columns).sum();
        if let Some(last) = row.last_mut() {
            last.columns += COLUMNS - taken;
        }
    }
    rows
}

fn text_item(x: f64, y: f64, text: &str, size: f64, color: u32) -> Value {
    json!({"type": ImageItemType::Text.value(), "x": x, "y": y, "text": text, "size": size, "color": color})
}

fn rect(x: f64, y: f64, width: f64, height: f64, radius: f64, color: u32) -> Value {
    json!({"type": ImageItemType::Rect.value(), "x": x, "y": y, "w": width, "h": height, "radius": radius, "color": color})
}

fn line_top(top: f64, size: f64) -> f64 {
    top + (TILE_HEIGHT - size * LINE_BOX) / 2.0
}

fn put_spans(items: &mut Vec<Value>, spans: &[Span], mut x: f64, y: f64) {
    for span in spans {
        items.push(text_item(x, y, &span.text, VALUE_SIZE, span.color));
        x += measure(&span.text, VALUE_SIZE, FontWeight::Regular);
    }
}

fn draw_tile(items: &mut Vec<Value>, tile: &Tile, x: f64, top: f64, height: f64) {
    let width = columns_width(tile.columns);
    items.push(rect(x, top, width, height, TILE_RADIUS, TILE));
    items.push(text_item(
        x + TILE_PADDING,
        line_top(top, LABEL_SIZE),
        tile.label,
        LABEL_SIZE,
        LABEL,
    ));
    if let [single] = tile.lines.as_slice() {
        let start = x + width - TILE_PADDING - spans_width(single);
        put_spans(items, single, start, line_top(top, VALUE_SIZE));
        return;
    }
    let start = x + TILE_PADDING + tile.label_width + LABEL_GAP;
    for (index, line) in tile.lines.iter().enumerate() {
        put_spans(items, line, start, line_top(top, VALUE_SIZE) + index as f64 * WRAP_LINE);
    }
}

fn status(environment: &Value) -> (String, u32, u32) {
    let failed = plugin_counts(environment).failed.len();
    let crashes = environment.get("crashes_24h").and_then(Value::as_i64).unwrap_or(0);
    if failed > 0 {
        (format!("● {failed} failing"), BAD, BAD_BACKGROUND)
    } else if crashes > 0 {
        ("● crashing".to_owned(), BAD, BAD_BACKGROUND)
    } else {
        ("● healthy".to_owned(), GOOD, GOOD_BACKGROUND)
    }
}

fn draw_header(items: &mut Vec<Value>, environment: &Value, stamp: &str) {
    let title_top = (HEADER_HEIGHT - TITLE_SIZE * LINE_BOX) / 2.0 + PADDING / 2.0;
    let mut title = text_item(PADDING + TILE_PADDING / 2.0, title_top, "astral", TITLE_SIZE, VALUE);
    title["weight"] = json!(FontWeight::Bold.value());
    items.push(title);
    let (label, color, background) = status(environment);
    let pill_width = measure(&label, META_SIZE, FontWeight::Semibold) + PILL_PADDING * 2.0;
    let pill_x = WIDTH - PADDING - pill_width;
    let pill_top = (HEADER_HEIGHT - PILL_HEIGHT) / 2.0 + PADDING / 2.0;
    items.push(rect(pill_x, pill_top, pill_width, PILL_HEIGHT, PILL_HEIGHT / 2.0, background));
    let mut pill = text_item(
        pill_x,
        pill_top + (PILL_HEIGHT - META_SIZE * LINE_BOX) / 2.0,
        &label,
        META_SIZE,
        color,
    );
    pill["width"] = json!(pill_width);
    pill["weight"] = json!(FontWeight::Semibold.value());
    pill["align"] = json!(TextAlign::Center.value());
    items.push(pill);
    let version = present(environment, "astral").unwrap_or_else(|| "?".to_owned());
    let mut meta = vec![span(version, DIM)];
    if let Some(commit) = present(environment, "commit") {
        meta.push(span(format!(" · @{commit}"), COMMIT));
    }
    meta.push(span(format!(" · {stamp}"), DIM));
    let x = PADDING + TILE_PADDING / 2.0 + measure("astral", TITLE_SIZE, FontWeight::Bold) + LABEL_GAP;
    let room = pill_x - LABEL_GAP - x;
    let meta_width = |meta: &[Span]| -> f64 { meta.iter().map(|span| measure(&span.text, META_SIZE, FontWeight::Regular)).sum() };
    while meta.len() > 1 && meta_width(&meta) > room {
        meta.pop();
    }
    let meta_top = title_top + (TITLE_SIZE - META_SIZE) * META_BASELINE;
    let mut item_x = x;
    for span in &meta {
        items.push(text_item(item_x, meta_top, &span.text, META_SIZE, span.color));
        item_x += measure(&span.text, META_SIZE, FontWeight::Regular);
    }
}

pub fn card(environment: &Value, stamp: &str) -> Value {
    let tiles: Vec<Tile> = rows(environment)
        .into_iter()
        .map(|(label, spans)| tile(label, spans.into_iter().filter(|span| !span.text.is_empty()).collect()))
        .collect();
    let mut items = Vec::new();
    draw_header(&mut items, environment, stamp);
    let mut top = HEADER_HEIGHT + PADDING / 2.0;
    for row in packed(tiles) {
        let height = row.iter().map(tile_height).fold(TILE_HEIGHT, f64::max);
        let mut x = PADDING;
        for tile in &row {
            draw_tile(&mut items, tile, x, top, height);
            x += columns_width(tile.columns) + GAP;
        }
        top += height + GAP;
    }
    let height = top - GAP + PADDING;
    items.insert(0, rect(0.0, 0.0, WIDTH, height, CARD_RADIUS, CARD));
    json!({"width": WIDTH, "height": height, "scale": SCALE, "items": items})
}

fn alt_payload(environment: &Value) -> Map<String, Value> {
    let discord = discord_parts(environment);
    let counts = plugin_counts(environment);
    let mut payload = Map::new();
    let mut put = |key: &str, value: Option<Value>| {
        if let Some(value) = value.filter(|value| !value.is_null()) {
            payload.insert(key.to_owned(), value);
        }
    };
    let field = |key: &str| environment.get(key).cloned();
    put("astral", field("astral"));
    put("commit", field("commit"));
    put("loader", field("image"));
    put("framework", field("framework"));
    put("jailbreak", field("jailbreak"));
    put("discord", Some(json!(discord.version)));
    put("channel", discord.channel.map(Value::String));
    put("build", discord.build.map(Value::String));
    put("support", field("compatibility"));
    put("source", field("source"));
    put(
        "install",
        present(environment, "install").map(|install| json!(install.replace('`', ""))),
    );
    put("device", field("device_name").or_else(|| field("device")));
    put("system", field("system"));
    put("signer", field("signer"));
    put("binary", field("binary"));
    put("markers", field("markers"));
    put("https", field("https"));
    put("phoenix", field("phoenix"));
    put("plugins", Some(json!(format!("{}/{}", counts.enabled, counts.installed))));
    put("plugins_failed", (!counts.failed.is_empty()).then(|| json!(counts.failed)));
    put("crash", field("last_report"));
    payload
}

fn alt_text(environment: &Value) -> String {
    let mut payload = alt_payload(environment);
    let mut text = Value::Object(payload.clone()).to_string();
    for key in ALT_DROP_ORDER {
        if text.len() <= MAX_ALT_TEXT {
            break;
        }
        payload.remove(key);
        text = Value::Object(payload.clone()).to_string();
    }
    let end = (0..=MAX_ALT_TEXT.min(text.len()))
        .rev()
        .find(|end| text.is_char_boundary(*end))
        .unwrap_or(0);
    text[..end].to_owned()
}

fn stamp() -> String {
    let milliseconds = crate::support::bridge::epoch_ms();
    let mut iso: String = crate::lua::host::api::gateway_timestamp(milliseconds).chars().take(16).collect();
    iso.replace_range(10..11, " ");
    format!("{iso} utc")
}

pub fn environment() -> Value {
    let mut environment = reports::environment();
    environment["plugin_list"] = crate::lua::host::plugin_list();
    environment
}

fn sanitized(text: &str) -> String {
    crate::diagnostics::privacy::sanitize(text.as_bytes())
}

fn newest(text: String) -> String {
    if text.len() <= LOG_LIMIT {
        return text;
    }
    let start = text.len() - LOG_LIMIT;
    let start = text[start..].find('\n').map_or(start, |offset| start + offset + 1);
    text[start..].to_owned()
}

fn state_files(state: &str) -> Vec<(String, String)> {
    let Ok(entries) = std::fs::read_dir(state) else { return Vec::new() };
    let mut files: Vec<(String, String)> = entries
        .flatten()
        .filter(|entry| {
            entry
                .metadata()
                .is_ok_and(|metadata| metadata.is_file() && metadata.len() <= STATE_LIMIT)
        })
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if PRIVATE_STATE.iter().any(|private| name.starts_with(private)) {
                return None;
            }
            let text = std::fs::read_to_string(entry.path()).ok()?;
            Some((name, sanitized(&text)))
        })
        .collect();
    files.sort();
    files
}

fn entitlements() -> Option<String> {
    let facts: Value = host::request(Request::EnvironmentFacts)
        .into_text()
        .and_then(|text| serde_json::from_str(&text).ok())?;
    let blob = crate::lua::host::install::decode_base64(facts.get("kernelEntitlements")?.as_str()?)?;
    let plist = crate::diagnostics::macho::entitlements_from_blob(&blob)?;
    Some(String::from_utf8_lossy(&plist).into_owned())
}

fn diagnostics_zip(environment: &Value) -> Option<Vec<u8>> {
    let current = app()?;
    let mut files: Vec<(String, String)> = vec![
        ("debug-info.json".into(), serde_json::to_string_pretty(environment).ok()?),
        (
            "logs/astral.log".into(),
            newest(sanitized(&crate::support::dev_server::log_lines().join("\n"))),
        ),
        (
            "plugins.json".into(),
            serde_json::to_string_pretty(&crate::lua::host::plugins_json()).ok()?,
        ),
    ];
    for (name, text) in state_files(&current.state) {
        files.push((format!("state/{name}"), text));
    }
    for (index, report) in reports::list().as_array().into_iter().flatten().enumerate() {
        if let Some(detail) = report.get("detail").and_then(Value::as_str).filter(|detail| !detail.is_empty()) {
            files.push((format!("crashes/{index}.txt"), sanitized(detail)));
        }
    }
    if current.platform == Platform::Ios
        && let Some(entitlements) = entitlements()
    {
        files.push(("Entitlements.xml".into(), entitlements));
    }
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, text) in files {
        zip.start_file(name, options).ok()?;
        zip.write_all(text.as_bytes()).ok()?;
    }
    zip.finish().ok().map(std::io::Cursor::into_inner)
}

pub struct Report {
    pub card: Option<Vec<u8>>,
    pub alt: String,
    pub diagnostics: Option<Vec<u8>>,
}

pub fn report(diagnostics: bool) -> Report {
    let environment = environment();
    let card = card(&environment, &stamp());
    Report {
        card: crate::render::image::render(&card),
        alt: alt_text(&environment),
        diagnostics: diagnostics.then(|| diagnostics_zip(&environment)).flatten(),
    }
}

fn execute(options: &Value, _: &CommandContext) -> CommandReply {
    let note = options.get("message").and_then(Value::as_str).unwrap_or_default().trim().to_owned();
    let report = report(options.get("diagnostics").and_then(Value::as_bool) == Some(true));
    let mut attachments = Vec::new();
    if let Some(png) = report.card {
        attachments.push(Attachment {
            name: CARD_NAME.into(),
            content_type: PNG.into(),
            description: Some(report.alt),
            data: png,
        });
    }
    if let Some(zip) = report.diagnostics {
        attachments.push(Attachment {
            name: format!("astral-diagnostics-{}.zip", crate::support::bridge::epoch_ms()),
            content_type: ZIP.into(),
            description: None,
            data: zip,
        });
    }
    if attachments.is_empty() {
        return CommandReply::Local(json!({ "content": "Couldn't build the debug report on this device." }));
    }
    CommandReply::Send {
        message: json!({ "content": note }),
        attachments,
    }
}

pub fn register() {
    register_native(NativeCommand {
        name: "debug",
        description: "Posts astral's debug card for support",
        options: json!([
            {"name": "message", "description": "Add a note above the card", "type": OptionType::String.discord_type()},
            {"name": "diagnostics", "description": "Attach a zip of astral's logs, crash records and state", "type": OptionType::Boolean.discord_type()},
        ]),
        execute,
    });
}

#[cfg(test)]
mod tests {
    use super::{alt_text, binary_text, card, framework_version};
    use serde_json::json;

    #[test]
    fn trims_noise() {
        assert_eq!(binary_text("encrypted · min 16.0 · 107 dylibs · UUID 2F7"), "encrypted · min 16.0");
        assert_eq!(framework_version("ElleKit 1.1.3-2"), "ElleKit 1.1.3");
    }

    #[test]
    fn draws_the_card_and_keeps_alt_text_short() {
        let environment = json!({
            "astral": "0.0.1", "discord": "357.0 (124376)", "discord_channel": "canary", "compatibility": "ready",
            "commit": "4bb262921", "image": "astral.dylib", "markers": ["120 Hz", "Developer Mode on"],
            "plugin_list": [{"id": "astral.translate", "name": "Translate", "enabled": true, "failed": true}],
            "binary": "x".repeat(2000),
        });
        let drawn = card(&environment, "2026-11-26 09:00 utc");
        let texts: Vec<&str> = drawn["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|item| item["text"].as_str())
            .collect();
        assert!(texts.contains(&"alpha · 124376") || texts.iter().any(|text| text.contains("alpha · 124376")));
        assert!(texts.contains(&"Developer Mode on"));
        assert!(texts.iter().any(|text| text.contains("1 failing")));
        assert!(alt_text(&environment).len() <= 1024);
    }
}
