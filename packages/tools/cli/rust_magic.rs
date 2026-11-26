use regex::Regex;

use crate::repo;

const DECODER_PREFIXES: [&str; 3] = ["parse", "from_", "decode"];

pub struct Finding {
    pub line: usize,
    pub text: String,
}

fn without_strings(line: &str) -> String {
    let strings = Regex::new(r#""(?:[^"\\]|\\.)*"|b'(?:[^'\\]|\\.)'|'(?:[^'\\]|\\.)'"#)
        .expect("string pattern");
    strings.replace_all(line, "\"\"").into_owned()
}

fn is_declaration(code: &str) -> bool {
    let trimmed = code.trim_start();
    [
        "const ",
        "static ",
        "pub const ",
        "pub static ",
        "pub(crate) const ",
    ]
    .iter()
    .any(|prefix| trimmed.starts_with(prefix))
}

pub fn findings(source: &str) -> Vec<Finding> {
    let function = Regex::new(r"\bfn (\w+)").expect("function pattern");
    let comparison = Regex::new(r"(?:^|[^\w.])[=!]=\s*-?(?:0x[0-9a-fA-F_]+|[3-9]|\d{2,})\b")
        .expect("comparison pattern");
    let arm = Regex::new(r"^\s+-?(?:0x[0-9a-fA-F_]+|[3-9]|\d{2,})(?:\s*\|\s*-?(?:0x[0-9a-fA-F_]+|\d+))*(?:\s+if .*)?\s*=>")
        .expect("arm pattern");
    let mut current = String::new();
    let mut found = Vec::new();
    for (index, line) in source.lines().enumerate() {
        if line.trim_start().starts_with("mod tests") {
            break;
        }
        if let Some(name) = function.captures(line) {
            current = name[1].to_owned();
        }
        if DECODER_PREFIXES
            .iter()
            .any(|prefix| current.starts_with(prefix))
        {
            continue;
        }
        let code = without_strings(line);
        if is_declaration(&code) {
            continue;
        }
        if comparison.is_match(&code) || arm.is_match(&code) {
            found.push(Finding {
                line: index + 1,
                text: line.trim().to_owned(),
            });
        }
    }
    found
}

pub fn check(files: &[String]) -> Result<(), String> {
    let mut problems = Vec::new();
    for file in files.iter().filter(|file| !file.contains("/tests/")) {
        let source = repo::read(&repo::path(&[file]))?;
        for finding in findings(&source) {
            problems.push(format!(
                "{file}:{}: name this number with a const or an enum: {}",
                finding.line, finding.text
            ));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(format!("rust magic numbers:\n{}", problems.join("\n")))
    }
}

pub fn check_all() -> Result<(), String> {
    check(&repo::tracked_files(&[
        "packages/core/*.rs",
        "packages/tools/*.rs",
    ])?)
}
