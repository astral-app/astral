use std::collections::BTreeMap;

use astral_core::lua_enums::{ALL, ALL_NUMBERS};
use regex::Regex;

use crate::repo;

const SDK: [&str; 4] = ["packages", "sdk", "library", "astral.lua"];
const SECTION_END: &str = "---@enum astral.Permission";
const ASSIGN_END: &str = "astral.Permission = Permission";

const FIELD_ENUMS: &[(&str, &[&str])] = &[
    (
        "type",
        &[
            "SheetItemType",
            "PatchNodeType",
            "InsertNodeType",
            "ImageItemType",
            "OptionType",
            "PickedMediaType",
        ],
    ),
    (
        "align",
        &["ScrollAlign", "Align", "TextAlign", "SheetAlign"],
    ),
    ("justify", &["Justify"]),
    ("position", &["InsertPosition"]),
    ("reply", &["ReplyPlacement"]),
    ("time", &["TimePlacement"]),
    ("name", &["NamePlacement"]),
    ("weight", &["FontWeight"]),
    ("style", &["ButtonStyle"]),
    ("direction", &["Direction"]),
    ("kind", &["MediaKind", "NodeKind", "NameColorKind"]),
    ("color", &["SheetColor"]),
];

const CALL_ENUMS: &[(&str, &str)] = &[
    ("astral.events.on", "Event"),
    ("astral.ui.decorate", "Target"),
    ("astral.ui.relabel", "Target"),
];

fn blocks() -> Vec<(String, Vec<(String, String)>)> {
    let strings = ALL.iter().map(|(name, members)| {
        let members = members
            .iter()
            .map(|(member, value)| ((*member).to_owned(), format!("{value:?}")))
            .collect();
        ((*name).to_owned(), members)
    });
    let numbers = ALL_NUMBERS.iter().map(|(name, members)| {
        let members = members
            .iter()
            .map(|(member, value)| ((*member).to_owned(), value.to_string()))
            .collect();
        ((*name).to_owned(), members)
    });
    strings.chain(numbers).collect()
}

pub fn sdk_section() -> String {
    blocks()
        .iter()
        .map(|(name, members)| {
            let lines: String = members
                .iter()
                .map(|(member, value)| format!("\t{member} = {value},\n"))
                .collect();
            format!("---@enum astral.{name}\nlocal {name} = {{\n{lines}}}\n")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn sdk_assignments() -> String {
    blocks()
        .iter()
        .map(|(name, _)| format!("astral.{name} = {name}\n"))
        .collect()
}

fn replace_between(
    source: &str,
    start: &str,
    end: &str,
    replacement: &str,
) -> Result<String, String> {
    let from = source
        .find(start)
        .ok_or_else(|| format!("the SDK has no {start:?}"))?;
    let to = source[from..]
        .find(end)
        .map(|offset| from + offset)
        .ok_or_else(|| format!("the SDK has no {end:?}"))?;
    Ok(format!("{}{replacement}{}", &source[..from], &source[to..]))
}

pub fn rendered_sdk(source: &str) -> Result<String, String> {
    let first = blocks()
        .first()
        .map(|(name, _)| name.clone())
        .unwrap_or_default();
    let section = format!("{}\n", sdk_section());
    let with_section = replace_between(
        source,
        &format!("---@enum astral.{first}\n"),
        SECTION_END,
        &section,
    )?;
    replace_between(
        &with_section,
        &format!("astral.{first} = {first}\n"),
        ASSIGN_END,
        &sdk_assignments(),
    )
}

pub fn sync_sdk(check: bool) -> Result<(), String> {
    let path = repo::path(&SDK);
    let source = repo::read(&path)?;
    let rendered = rendered_sdk(&source)?;
    if rendered == source {
        return Ok(());
    }
    if check {
        return Err(
            "the SDK enums differ from packages/core/lua_enums.rs; run astral sdk enums".into(),
        );
    }
    std::fs::write(&path, rendered)
        .map_err(|error| format!("Failed to write {}: {error}", path.display()))
}

fn values() -> BTreeMap<&'static str, Vec<(&'static str, &'static str)>> {
    ALL.iter()
        .map(|(name, members)| (*name, members.to_vec()))
        .collect()
}

fn suggestions(enums: &[&str], value: &str) -> Vec<String> {
    let known = values();
    enums
        .iter()
        .filter_map(|name| {
            known
                .get(name)
                .and_then(|members| members.iter().find(|(_, candidate)| *candidate == value))
                .map(|(member, _)| format!("astral.{name}.{member}"))
        })
        .collect()
}

pub struct Finding {
    pub line: usize,
    pub literal: String,
    pub suggestions: Vec<String>,
}

pub fn findings(source: &str) -> Vec<Finding> {
    let assignment =
        Regex::new(r#"\b([a-z_]+)\s*(?:=|==|~=)\s*"([a-z_.]+)""#).expect("assignment pattern");
    let call = Regex::new(r#"(astral\.[a-z_.]+)\(\s*"([a-z_.]+)""#).expect("call pattern");
    let mut found = Vec::new();
    for (index, line) in source.lines().enumerate() {
        for capture in assignment.captures_iter(line) {
            let Some((_, enums)) = FIELD_ENUMS.iter().find(|(field, _)| *field == &capture[1])
            else {
                continue;
            };
            let suggestions = suggestions(enums, &capture[2]);
            if !suggestions.is_empty() {
                found.push(Finding {
                    line: index + 1,
                    literal: capture[2].to_owned(),
                    suggestions,
                });
            }
        }
        for capture in call.captures_iter(line) {
            let Some((_, enum_name)) = CALL_ENUMS
                .iter()
                .find(|(function, _)| *function == &capture[1])
            else {
                continue;
            };
            let suggestions = suggestions(&[enum_name], &capture[2]);
            if !suggestions.is_empty() {
                found.push(Finding {
                    line: index + 1,
                    literal: capture[2].to_owned(),
                    suggestions,
                });
            }
        }
    }
    found
}

pub fn check(files: &[String]) -> Result<(), String> {
    let mut problems = Vec::new();
    for file in files {
        let source = repo::read(&repo::path(&[file]))?;
        for finding in findings(&source) {
            problems.push(format!(
                "{file}:{}: use {} instead of the string {:?}",
                finding.line,
                finding.suggestions.join(" or "),
                finding.literal
            ));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(format!("lua enums:\n{}", problems.join("\n")))
    }
}

pub fn check_plugins() -> Result<(), String> {
    check(&repo::tracked_files(&["packages/addons/plugins/*.lua"])?)
}
