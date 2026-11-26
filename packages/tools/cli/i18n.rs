use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;

const SOURCE_EXTENSION: &str = "json";
const TARGET_EXTENSION: &str = "toml";

fn read_messages(path: &Path) -> Result<BTreeMap<String, String>, String> {
    let text =
        std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let parsed: Value =
        serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))?;
    let object = parsed
        .as_object()
        .ok_or_else(|| format!("{}: not an object", path.display()))?;
    Ok(object
        .iter()
        .filter_map(|(key, value)| value.as_str().map(|text| (key.clone(), text.to_owned())))
        .collect())
}

pub fn toml_messages(messages: &BTreeMap<String, String>) -> String {
    messages
        .iter()
        .map(|(key, text)| format!("{key} = {}\n", toml::Value::String(text.clone())))
        .collect()
}

fn keep(messages: BTreeMap<String, String>, keys: Option<&[String]>) -> BTreeMap<String, String> {
    match keys {
        Some(keys) => messages
            .into_iter()
            .filter(|(key, _)| keys.contains(key))
            .collect(),
        None => messages,
    }
}

pub fn import(source: &Path, target: &Path, keys: Option<&[String]>) -> Result<usize, String> {
    let entries =
        std::fs::read_dir(source).map_err(|error| format!("{}: {error}", source.display()))?;
    std::fs::create_dir_all(target).map_err(|error| format!("{}: {error}", target.display()))?;
    let mut written = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some(SOURCE_EXTENSION) {
            continue;
        }
        let messages = keep(read_messages(&path)?, keys);
        if messages.is_empty() {
            continue;
        }
        let name = path.with_extension(TARGET_EXTENSION);
        let file = target.join(name.file_name().unwrap_or_default());
        std::fs::write(&file, toml_messages(&messages))
            .map_err(|error| format!("{}: {error}", file.display()))?;
        written += 1;
    }
    Ok(written)
}

pub fn run(arguments: &[String]) -> Result<(), String> {
    let [command, source, target, rest @ ..] = arguments else {
        return Err(
            "usage: astral i18n import <unbound i18n dir> <plugin i18n dir> [--keys A,B]".into(),
        );
    };
    if command != "import" {
        return Err(format!("unknown i18n command {command}"));
    }
    let keys: Option<Vec<String>> = match rest {
        [flag, list] if flag == "--keys" => Some(list.split(',').map(str::to_owned).collect()),
        [] => None,
        _ => {
            return Err(
                "usage: astral i18n import <unbound i18n dir> <plugin i18n dir> [--keys A,B]"
                    .into(),
            );
        }
    };
    let written = import(Path::new(source), Path::new(target), keys.as_deref())?;
    println!("wrote {written} locales to {target}");
    Ok(())
}
