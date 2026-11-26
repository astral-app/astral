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
const PADDING: f64 = 12.0;
const HEADER_HEIGHT: f64 = 34.0;
const FONT_SIZE: f64 = 11.5;
const CHAR_WIDTH: f64 = FONT_SIZE * 0.6;
const LINE_HEIGHT: f64 = 17.0;
const LABEL_WIDTH: f64 = 66.0;
const LABEL_SIZE: f64 = 11.0;
const ALT_CLEARANCE: f64 = 64.0;
const ALT_HEIGHT: f64 = 46.0;
const CARD_RADIUS: f64 = 12.0;
const STRIPE_INSET: f64 = 6.0;
const STRIPE_RADIUS: f64 = 4.0;
const TITLE_SIZE: f64 = 17.0;
const TITLE_WIDTH: f64 = 60.0;
const TITLE_ADVANCE: f64 = 56.0;
const PILL_HEIGHT: f64 = 17.0;
const PILL_SIZE: f64 = 10.0;
const PILL_GLYPH: f64 = 0.62;
const PILL_PADDING: f64 = 14.0;
const PILL_GAP: f64 = 6.0;
const STAMP_SIZE: f64 = 10.0;
const MONO_GLYPH: f64 = 0.6;
const MAX_SPANS: usize = 8;
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

const CARD: u32 = 0xFF1E_1F22;
const STRIPE: u32 = 0xFF23_2428;
const LABEL: u32 = 0xFF94_9BA4;
const VALUE: u32 = 0xFFF2_F3F5;
const DIM: u32 = 0xFF94_9BA4;
const ACCENT: u32 = 0xFF58_65F2;
const COMMIT: u32 = 0xFFA7_8BFA;
const GOOD: u32 = 0xFF23_A55A;
const WARN: u32 = 0xFFF0_B232;
const BAD: u32 = 0xFFDA_373C;

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

fn length(text: &str) -> usize {
    text.chars().count()
}

fn spans_length(spans: &[Span]) -> usize {
    spans.iter().map(|span| length(&span.text)).sum()
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

fn kept_spans(spans: &[Span]) -> Vec<Span> {
    spans.iter().take(MAX_SPANS).filter(|span| !span.text.is_empty()).cloned().collect()
}

fn byte_offset(text: &str, characters: usize) -> usize {
    text.char_indices().nth(characters).map_or(text.len(), |(offset, _)| offset)
}

fn wrapped(spans: &[Span], room: usize) -> Vec<Vec<Span>> {
    let mut lines = Vec::new();
    let mut line: Vec<Span> = Vec::new();
    let mut used = 0;
    for item in spans {
        let mut rest = item.text.clone();
        while !rest.is_empty() {
            let left = room.saturating_sub(used);
            if length(&rest) <= left {
                used += length(&rest);
                line.push(span(std::mem::take(&mut rest), item.color));
                continue;
            }
            let cut = byte_offset(&rest, left);
            let mut head = rest[..cut].to_owned();
            if !rest[cut..].starts_with(' ')
                && let Some(space) = head.rfind(' ').filter(|space| *space > 0)
            {
                head.truncate(space);
            }
            if head.is_empty() && used > 0 {
                lines.push(std::mem::take(&mut line));
                used = 0;
                continue;
            }
            rest = rest[head.len()..].strip_prefix(' ').unwrap_or(&rest[head.len()..]).to_owned();
            if head.is_empty() {
                head = rest.chars().next().map(String::from).unwrap_or_default();
                rest = rest[head.len()..].to_owned();
            }
            line.push(span(head, item.color));
            lines.push(std::mem::take(&mut line));
            used = 0;
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

fn text_item(x: f64, y: f64, text: &str, size: f64, color: u32) -> Value {
    json!({"type": ImageItemType::Text.value(), "x": x, "y": y, "text": text, "size": size, "color": color})
}

fn runs(items: &mut Vec<Value>, spans: &[Span], mut x: f64, y: f64) {
    for span in spans {
        let mut item = text_item(x, y, &span.text, FONT_SIZE, span.color);
        item["mono"] = json!(true);
        items.push(item);
        x += length(&span.text) as f64 * CHAR_WIDTH;
    }
}

fn pill(items: &mut Vec<Value>, x: f64, y: f64, label: &str, color: u32) -> f64 {
    let width = (length(label) as f64 * PILL_SIZE * PILL_GLYPH).ceil() + PILL_PADDING;
    items.push(json!({"type": ImageItemType::Rect.value(), "x": x, "y": y, "w": width, "h": PILL_HEIGHT, "radius": PILL_HEIGHT / 2.0, "color": color}));
    let mut item = text_item(x, y + 2.5, label, PILL_SIZE, VALUE);
    item["width"] = json!(width);
    item["weight"] = json!(FontWeight::Bold.value());
    item["align"] = json!(TextAlign::Center.value());
    items.push(item);
    width
}

fn summary_spans(environment: &Value) -> Vec<Span> {
    let counts = plugin_counts(environment);
    let mut spans = Vec::new();
    if let Some(jailbreak) = present(environment, "jailbreak") {
        spans.push(span(format!("{}  ", jailbreak.to_lowercase()), DIM));
    }
    let crashes = environment.get("crashes_24h").and_then(Value::as_i64).unwrap_or(0);
    spans.push(if !counts.failed.is_empty() {
        span(format!("● {} failing  ", counts.failed.len()), BAD)
    } else if crashes > 0 {
        span("● crashing  ", BAD)
    } else {
        span("● healthy  ", GOOD)
    });
    spans.push(span(format!("{}/{} plugins", counts.enabled, counts.installed), DIM));
    spans
}

fn draw_header(items: &mut Vec<Value>, environment: &Value, stamp: &str) {
    let mut title = text_item(PADDING, PADDING - 3.0, "astral", TITLE_SIZE, VALUE);
    title["width"] = json!(TITLE_WIDTH);
    title["weight"] = json!(FontWeight::Bold.value());
    items.push(title);
    let version = present(environment, "astral").unwrap_or_else(|| "?".to_owned());
    let mut x = PADDING + TITLE_ADVANCE + pill(items, PADDING + TITLE_ADVANCE, PADDING, &version, ACCENT) + PILL_GAP;
    let commit = present(environment, "commit").map(|commit| span(format!("@{commit}"), COMMIT));
    let mut trailing: Vec<Span> = commit.iter().cloned().collect();
    trailing.push(span(format!("{}{stamp}", if commit.is_some() { "  " } else { "" }), DIM));
    let room = ((WIDTH - PADDING - x) / (STAMP_SIZE * MONO_GLYPH)).floor() as usize;
    if spans_length(&trailing) > room {
        trailing.truncate(1);
    }
    for span in &trailing {
        let mut item = text_item(x, PADDING + 3.0, &span.text, STAMP_SIZE, span.color);
        item["mono"] = json!(true);
        items.push(item);
        x += length(&span.text) as f64 * STAMP_SIZE * MONO_GLYPH;
    }
}

struct Line {
    label: &'static str,
    spans: Vec<Span>,
    stripe: bool,
}

pub fn card(environment: &Value, stamp: &str) -> Value {
    let room = ((WIDTH - PADDING * 2.0 - LABEL_WIDTH) / CHAR_WIDTH).floor() as usize;
    let mut lines = Vec::new();
    for (index, (label, spans)) in rows(environment).into_iter().enumerate() {
        for (line_index, spans) in wrapped(&kept_spans(&spans), room).into_iter().enumerate() {
            lines.push(Line {
                label: if line_index == 0 { label } else { "" },
                spans,
                stripe: index % 2 == 1,
            });
        }
    }
    let summary = summary_spans(environment);
    let summary_room = ((WIDTH - PADDING * 2.0 - ALT_CLEARANCE) / CHAR_WIDTH).floor() as usize;
    let height = HEADER_HEIGHT + lines.len() as f64 * LINE_HEIGHT + ALT_HEIGHT;
    let mut items =
        vec![json!({"type": ImageItemType::Rect.value(), "x": 0, "y": 0, "w": WIDTH, "h": height, "radius": CARD_RADIUS, "color": CARD})];
    draw_header(&mut items, environment, stamp);
    for (index, line) in lines.iter().enumerate() {
        let top = HEADER_HEIGHT + index as f64 * LINE_HEIGHT;
        if line.stripe {
            items.push(json!({
                "type": ImageItemType::Rect.value(), "x": PADDING - STRIPE_INSET, "y": top,
                "w": WIDTH - (PADDING - STRIPE_INSET) * 2.0, "h": LINE_HEIGHT, "radius": STRIPE_RADIUS, "color": STRIPE,
            }));
        }
        let mut label = text_item(PADDING, top + 2.5, line.label, LABEL_SIZE, LABEL);
        label["width"] = json!(LABEL_WIDTH - STRIPE_INSET);
        items.push(label);
        runs(&mut items, &line.spans, PADDING + LABEL_WIDTH, top + 2.0);
    }
    if spans_length(&summary) <= summary_room {
        let top = HEADER_HEIGHT + lines.len() as f64 * LINE_HEIGHT;
        let x = WIDTH - PADDING - spans_length(&summary) as f64 * CHAR_WIDTH;
        runs(&mut items, &summary, x, top + (ALT_HEIGHT - FONT_SIZE) / 2.0 - 2.0);
    }
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
    use super::{VALUE, alt_text, binary_text, card, framework_version, span, wrapped};
    use serde_json::json;

    #[test]
    fn wraps_and_trims_noise() {
        let lines = wrapped(&[span("one two three", VALUE)], 7);
        let texts: Vec<String> = lines
            .iter()
            .map(|line| line.iter().map(|span| span.text.clone()).collect())
            .collect();
        assert_eq!(texts, vec!["one two", "three"]);
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
