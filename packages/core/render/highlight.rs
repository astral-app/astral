use crate::lua::enums::TextStyle;
use core::ffi::c_char;
use std::str::FromStr;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use syntect::easy::HighlightLines;
use syntect::highlighting::{Color, FontStyle, ScopeSelectors, StyleModifier, Theme, ThemeItem, ThemeSettings};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

use crate::support::bridge::{AstralRustString, bytes};

const MAX_CODE: usize = 6000;
const BUDGET: Duration = Duration::from_millis(500);
const THEME_CACHE: usize = 8;
const BOLD: u32 = TextStyle::Bold.value() as u32;
const ITALIC: u32 = TextStyle::Italic.value() as u32;
const UNDERLINE: u32 = TextStyle::Underline.value() as u32;

static SYNTAXES: OnceLock<SyntaxSet> = OnceLock::new();
static THEMES: Mutex<Vec<(u64, Arc<ParsedTheme>)>> = Mutex::new(Vec::new());

struct ParsedTheme {
    theme: Theme,
    light: bool,
}

fn syntaxes() -> &'static SyntaxSet {
    SYNTAXES.get_or_init(two_face::syntax::extra_newlines)
}

pub fn parse_color(text: &str) -> Option<Color> {
    let hex = text.strip_prefix('#')?;
    let digit = |index: usize, width: usize| u8::from_str_radix(&hex[index..index + width], 16).ok();
    let doubled = |index: usize| digit(index, 1).map(|value| value * 17);
    match hex.len() {
        3 => Some(Color {
            r: doubled(0)?,
            g: doubled(1)?,
            b: doubled(2)?,
            a: 255,
        }),
        4 => Some(Color {
            r: doubled(0)?,
            g: doubled(1)?,
            b: doubled(2)?,
            a: doubled(3)?,
        }),
        6 => Some(Color {
            r: digit(0, 2)?,
            g: digit(2, 2)?,
            b: digit(4, 2)?,
            a: 255,
        }),
        8 => Some(Color {
            r: digit(0, 2)?,
            g: digit(2, 2)?,
            b: digit(4, 2)?,
            a: digit(6, 2)?,
        }),
        _ => None,
    }
}

fn argb(color: Color) -> u32 {
    (u32::from(color.a) << 24) | (u32::from(color.r) << 16) | (u32::from(color.g) << 8) | u32::from(color.b)
}

fn font_style(text: &str) -> FontStyle {
    text.split_whitespace().fold(FontStyle::empty(), |style, word| match word {
        "bold" => style | FontStyle::BOLD,
        "italic" => style | FontStyle::ITALIC,
        "underline" => style | FontStyle::UNDERLINE,
        _ => style,
    })
}

fn scope_text(scope: &Value) -> Option<String> {
    match scope {
        Value::String(text) => Some(text.clone()),
        Value::Array(parts) => Some(parts.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")),
        _ => None,
    }
}

fn parse_theme(source: &Value) -> ParsedTheme {
    let light = source.get("type").and_then(Value::as_str) == Some("light");
    let editor = |key: &str| {
        source
            .get("colors")
            .and_then(|colors| colors.get(key))
            .and_then(Value::as_str)
            .and_then(parse_color)
    };
    let mut foreground = editor("editor.foreground");
    let mut background = editor("editor.background");
    let mut scopes = Vec::new();
    let rules = source
        .get("tokenColors")
        .or_else(|| source.get("settings"))
        .and_then(Value::as_array);
    for rule in rules.into_iter().flatten() {
        let settings = rule.get("settings").cloned().unwrap_or(Value::Null);
        let color = |key: &str| settings.get(key).and_then(Value::as_str).and_then(parse_color);
        let Some(scope) = rule.get("scope").and_then(scope_text).filter(|scope| !scope.trim().is_empty()) else {
            foreground = foreground.or_else(|| color("foreground"));
            background = background.or_else(|| color("background"));
            continue;
        };
        let Ok(selectors) = ScopeSelectors::from_str(&scope) else {
            continue;
        };
        scopes.push(ThemeItem {
            scope: selectors,
            style: StyleModifier {
                foreground: color("foreground"),
                background: None,
                font_style: settings.get("fontStyle").and_then(Value::as_str).map(font_style),
            },
        });
    }
    let fallback = if light {
        Color {
            r: 0x33,
            g: 0x33,
            b: 0x33,
            a: 255,
        }
    } else {
        Color {
            r: 0xd4,
            g: 0xd4,
            b: 0xd4,
            a: 255,
        }
    };
    let canvas = if light {
        Color {
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        }
    } else {
        Color {
            r: 0x1e,
            g: 0x1e,
            b: 0x1e,
            a: 255,
        }
    };
    ParsedTheme {
        theme: Theme {
            name: source.get("name").and_then(Value::as_str).map(str::to_owned),
            author: None,
            settings: ThemeSettings {
                foreground: Some(foreground.unwrap_or(fallback)),
                background: Some(background.unwrap_or(canvas)),
                ..ThemeSettings::default()
            },
            scopes,
        },
        light,
    }
}

fn theme_for(source: &[u8]) -> Arc<ParsedTheme> {
    let key = source.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    });
    let mut cache = THEMES.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some((_, theme)) = cache.iter().find(|(cached, _)| *cached == key) {
        return theme.clone();
    }
    let parsed = Arc::new(parse_theme(&serde_json::from_slice(source).unwrap_or(Value::Null)));
    if cache.len() >= THEME_CACHE {
        cache.remove(0);
    }
    cache.push((key, parsed.clone()));
    parsed
}

pub fn find_syntax(language: &str) -> Option<&'static SyntaxReference> {
    let language = language.trim();
    if language.is_empty() {
        return None;
    }
    let set = syntaxes();
    set.find_syntax_by_token(language)
        .or_else(|| set.syntaxes().iter().find(|syntax| syntax.name.eq_ignore_ascii_case(language)))
}

fn flags(style: FontStyle) -> u32 {
    let mut flags = 0;
    if style.contains(FontStyle::BOLD) {
        flags |= BOLD;
    }
    if style.contains(FontStyle::ITALIC) {
        flags |= ITALIC;
    }
    if style.contains(FontStyle::UNDERLINE) {
        flags |= UNDERLINE;
    }
    flags
}

pub fn highlight(code: &str, language: &str, theme: &[u8]) -> Value {
    let parsed = theme_for(theme);
    let settings = &parsed.theme.settings;
    let foreground = settings.foreground.map_or(0, argb);
    let code = &code[..code
        .char_indices()
        .map(|(index, _)| index)
        .find(|index| *index > MAX_CODE)
        .unwrap_or(code.len())];
    let syntax = find_syntax(language);
    let mut lines = Vec::new();
    let started = Instant::now();
    let mut highlighter = syntax.map(|syntax| HighlightLines::new(syntax, &parsed.theme));
    for line in LinesWithEndings::from(code) {
        let plain = || json!([[line.trim_end_matches(['\n', '\r']), foreground, 0]]);
        let runs = match highlighter.as_mut() {
            Some(lines_highlighter) if started.elapsed() < BUDGET => match lines_highlighter.highlight_line(line, syntaxes()) {
                Ok(regions) => Value::Array(
                    regions
                        .into_iter()
                        .map(|(style, text)| (style, text.trim_end_matches(['\n', '\r'])))
                        .filter(|(_, text)| !text.is_empty())
                        .map(|(style, text)| json!([text, argb(style.foreground), flags(style.font_style)]))
                        .collect(),
                ),
                Err(_) => plain(),
            },
            _ => plain(),
        };
        lines.push(runs);
    }
    if code.ends_with('\n') || code.is_empty() {
        lines.push(json!([]));
    }
    json!({
        "found": syntax.is_some(),
        "language": syntax.map_or("Plain Text", |syntax| syntax.name.as_str()),
        "foreground": foreground,
        "background": settings.background.map_or(0, argb),
        "light": parsed.light,
        "lines": lines,
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_code_highlight(
    code: *const c_char,
    code_length: usize,
    language: *const c_char,
    language_length: usize,
    theme: *const c_char,
    theme_length: usize,
) -> AstralRustString {
    let code = String::from_utf8_lossy(bytes(code, code_length));
    let language = String::from_utf8_lossy(bytes(language, language_length));
    AstralRustString::new(highlight(&code, &language, bytes(theme, theme_length)).to_string().into_bytes())
}
