use std::sync::LazyLock;

use regex::Regex;

use crate::repo;

static SNAKE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^_?[a-z][a-z0-9_]*$|^_$").expect("pattern"));
static UPPER_SNAKE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Z][A-Z0-9_]*$").expect("pattern"));
static PASCAL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Z][A-Za-z0-9]*$").expect("pattern"));
static LOCALS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\blocal\s+(function\s+)?([A-Za-z_][\w]*(?:\s*<\w+>)?(?:\s*,\s*[A-Za-z_][\w]*(?:\s*<\w+>)?)*)(\s*=\s*([\w.]+)(\s*\()?)?").expect("pattern")
});
static FUNCTIONS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bfunction\s+([\w.:]+)\s*\(([^)]*)\)").expect("pattern"));
static ANONYMOUS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bfunction\s*\(([^)]*)\)").expect("pattern"));
static LOOPS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bfor\s+([\w\s,]+?)\s+(?:in\b|=)").expect("pattern"));
const TYPE_FACTORIES: [&str; 3] = ["astral.class", "astral.enum", "setmetatable"];

#[derive(Debug, PartialEq)]
pub struct Finding {
    pub line: usize,
    pub name: String,
    pub kind: &'static str,
}

fn blank_keeping_lines(text: &str) -> String {
    "\n".repeat(text.matches('\n').count())
}

fn find_from(source: &str, from: usize, needle: &str) -> Option<usize> {
    source.get(from..)?.find(needle)
}

pub fn strip_lua_text(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut out = String::with_capacity(source.len());
    let mut index = 0;
    while index < bytes.len() {
        let rest = &source[index..];
        if rest.starts_with("--[[") {
            let end = find_from(source, index, "]]").map_or(source.len() - index, |end| end + 2);
            out.push_str(&blank_keeping_lines(
                &source[index..(index + end).min(source.len())],
            ));
            index += end;
        } else if rest.starts_with("--") {
            index += rest.find('\n').unwrap_or(rest.len());
        } else if rest.starts_with("[[") {
            let end =
                find_from(source, index + 2, "]]").map_or(source.len() - index, |end| end + 4);
            out.push_str("\"\"");
            out.push_str(&blank_keeping_lines(
                &source[index..(index + end).min(source.len())],
            ));
            index += end;
        } else if bytes[index] == b'"' || bytes[index] == b'\'' {
            let quote = bytes[index];
            let mut end = index + 1;
            while end < bytes.len() && bytes[end] != quote && bytes[end] != b'\n' {
                if bytes[end] == b'\\' {
                    end += 1;
                }
                end += 1;
            }
            out.push_str("\"\"");
            index = end + 1;
        } else {
            let character = rest.chars().next().unwrap_or_default();
            out.push(character);
            index += character.len_utf8();
        }
    }
    out
}

fn line_at(source: &str, offset: usize) -> usize {
    source[..offset.min(source.len())].matches('\n').count() + 1
}

fn split_names(list: &str) -> Vec<String> {
    list.split(',')
        .map(|part| {
            let name = part.trim();
            name.split('<').next().unwrap_or(name).trim().to_owned()
        })
        .filter(|name| !name.is_empty() && name != "..." && name != "self")
        .collect()
}

fn value_name(name: &str) -> bool {
    SNAKE.is_match(name) || UPPER_SNAKE.is_match(name)
}

fn annotated_type(lines: &[&str], line: usize) -> bool {
    for index in (0..line.saturating_sub(1)).rev() {
        let text = lines[index].trim();
        if text.starts_with("---@class") || text.starts_with("---@enum") {
            return true;
        }
        if !text.starts_with("---") {
            return false;
        }
    }
    false
}

pub fn findings(source: &str) -> Vec<Finding> {
    let code = strip_lua_text(source);
    let lines: Vec<&str> = source.split('\n').collect();
    let mut findings = Vec::new();
    for captures in LOCALS.captures_iter(&code) {
        let whole = captures.get(0).expect("match");
        let names = split_names(captures.get(2).map_or("", |names| names.as_str()));
        let typed = captures.get(4).is_some_and(|value| {
            let value = value.as_str();
            let last = value.rsplit('.').next().unwrap_or(value);
            (captures.get(5).is_some() && TYPE_FACTORIES.contains(&value))
                || (captures.get(5).is_none() && PASCAL.is_match(last))
        });
        for name in names {
            let class = PASCAL.is_match(&name)
                && (code.contains(&format!("{name}.__index"))
                    || annotated_type(&lines, line_at(&code, whole.start())));
            if value_name(&name) || class || (typed && PASCAL.is_match(&name)) {
                continue;
            }
            findings.push(Finding {
                line: line_at(&code, whole.start()),
                name,
                kind: "local",
            });
        }
    }
    for captures in FUNCTIONS.captures_iter(&code) {
        let start = captures.get(0).expect("match").start();
        let path = captures.get(1).map_or("", |path| path.as_str());
        if let Some(last) = path.split(['.', ':']).rfind(|part| !part.is_empty())
            && !SNAKE.is_match(last)
        {
            findings.push(Finding {
                line: line_at(&code, start),
                name: last.to_owned(),
                kind: "function",
            });
        }
        for name in split_names(captures.get(2).map_or("", |names| names.as_str())) {
            if !SNAKE.is_match(&name) {
                findings.push(Finding {
                    line: line_at(&code, start),
                    name,
                    kind: "parameter",
                });
            }
        }
    }
    for (pattern, kind) in [(&*ANONYMOUS, "parameter"), (&*LOOPS, "loop variable")] {
        for captures in pattern.captures_iter(&code) {
            let start = captures.get(0).expect("match").start();
            for name in split_names(captures.get(1).map_or("", |names| names.as_str())) {
                if !SNAKE.is_match(&name) {
                    findings.push(Finding {
                        line: line_at(&code, start),
                        name,
                        kind,
                    });
                }
            }
        }
    }
    findings
}

pub fn check(files: &[String]) -> Result<(), String> {
    let mut problems = Vec::new();
    for file in files {
        let source = repo::read(&repo::path(&[file]))?;
        for finding in findings(&source) {
            problems.push(format!(
                "{file}:{}: {} {:?} should be snake_case (UPPER_SNAKE for constants, PascalCase only for classes and enums)",
                finding.line, finding.kind, finding.name
            ));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(format!("lua casing:\n{}", problems.join("\n")))
    }
}
