use std::collections::BTreeMap;

use regex::Regex;

use crate::flags::Flags;
use crate::repo;

pub type Entry = BTreeMap<String, (String, String)>;

pub fn parse(source: &str) -> Result<(Vec<String>, BTreeMap<usize, Entry>), String> {
    let start = source
        .find("'SemanticColors': null")
        .ok_or("no SemanticColors in the source")?;
    let keys_pattern = Regex::new(r"r\d+ = \{('[A-Z0-9_]+': null(?:, )?)+\};").expect("pattern");
    let key_pattern = Regex::new(r"'([A-Z0-9_]+)': null").expect("pattern");
    let theme_pattern =
        Regex::new(r"= r\d+\.(ASH|LIGHT|ONYX|DARK|DARKER|MIDNIGHT);").expect("pattern");
    let raw_pattern =
        Regex::new(r"\{'raw': '([A-Z0-9_]+)', 'opacity': ([0-9.]+)\}").expect("pattern");
    let index_pattern = Regex::new(r"^(r\d+)\[(\d+)\] = (r\d+);$").expect("pattern");
    let entry_pattern = Regex::new(r"^(r\d+) = \{'category':").expect("pattern");
    let found = keys_pattern
        .find(&source[start..])
        .ok_or("no token list after SemanticColors")?;
    let keys: Vec<String> = key_pattern
        .captures_iter(found.as_str())
        .map(|captures| captures[1].to_owned())
        .collect();
    let mut entries = BTreeMap::new();
    let mut current = Entry::new();
    let (mut theme, mut register) = (String::new(), String::new());
    for line in source[start + found.end()..].split('\n').map(str::trim) {
        if let Some(captures) = entry_pattern.captures(line) {
            current = Entry::new();
            register = captures[1].to_owned();
            continue;
        }
        if let Some(captures) = theme_pattern.captures(line) {
            theme = captures[1].to_lowercase();
            continue;
        }
        if let Some(captures) = raw_pattern.captures(line)
            && !theme.is_empty()
        {
            current.insert(
                std::mem::take(&mut theme),
                (captures[1].to_owned(), captures[2].to_owned()),
            );
            continue;
        }
        if let Some(captures) = index_pattern.captures(line)
            && captures[3] == register
            && captures[1] != register
            && !current.is_empty()
        {
            let index: usize = captures[2].parse().unwrap_or(usize::MAX);
            if index >= keys.len() {
                break;
            }
            entries.insert(index, std::mem::take(&mut current));
            if entries.len() == keys.len() {
                break;
            }
        }
    }
    if entries.is_empty() {
        return Err("no semantic colour entries found".into());
    }
    Ok((keys, entries))
}

pub fn render(keys: &[String], entries: &BTreeMap<usize, Entry>, origin: &str) -> String {
    let mut output =
        format!("name = \"Discord semantic colours\"\nsource = {origin:?}\n\n[colors]\n");
    let mut indices: Vec<&usize> = entries.keys().collect();
    indices.sort_by_key(|index| &keys[**index]);
    for index in indices {
        let parts: Vec<String> = entries[index]
            .iter()
            .map(|(theme, (raw, opacity))| format!("{theme} = [{raw:?}, {opacity}]"))
            .collect();
        output.push_str(&format!("{} = {{ {} }}\n", keys[*index], parts.join(", ")));
    }
    output
}

pub fn run(arguments: &[String]) -> Result<(), String> {
    let usage = "usage: astral palette semantic --source <generated-definitions.tsx> --origin <description> [--output <toml>]";
    if arguments.first().map(String::as_str) != Some("semantic") {
        return Err(usage.into());
    }
    let default = repo::text(&repo::path(&[
        "packages",
        "addons",
        "themes",
        "palettes",
        "discord-semantic.toml",
    ]));
    let flags = Flags::new("palette")
        .string("source", "")
        .string("origin", "")
        .string("output", &default)
        .parse(&arguments[1..])?;
    if flags.get("source").is_empty() || flags.get("origin").is_empty() {
        return Err(usage.into());
    }
    let (keys, entries) = parse(&repo::read(std::path::Path::new(&flags.get("source")))?)?;
    std::fs::write(
        flags.get("output"),
        render(&keys, &entries, &flags.get("origin")),
    )
    .map_err(|error| error.to_string())?;
    println!(
        "wrote {} of {} semantic colours to {}",
        entries.len(),
        keys.len(),
        flags.get("output")
    );
    Ok(())
}
