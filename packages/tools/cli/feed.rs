use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::{Map, Value, json};

use crate::flags::Flags;
use crate::ipa_archive::{Ipa, text};
use crate::macho;

const MAX_VERSIONS: usize = 30;
const FIRST_PHOENIX_MAJOR: u64 = 357;
const PRIVACY_SUFFIX: &str = "UsageDescription";
const USAGE: &str = "usage: astral feed app-source --source <app-repo.json> --ipa <astral.ipa> --url <download URL> [--date <ISO date>] [--description <text>]";

#[derive(Debug, PartialEq)]
pub struct IpaFacts {
    pub version: String,
    pub build: String,
    pub minimum_os: String,
    pub size: u64,
    pub entitlements: BTreeSet<String>,
    pub privacy: BTreeMap<String, String>,
}

pub fn ipa_facts(path: &Path) -> Result<IpaFacts, String> {
    let size = std::fs::metadata(path)
        .map_err(|error| format!("{}: {error}", path.display()))?
        .len();
    let mut ipa = Ipa::open(path)?;
    let app = ipa.app()?;
    let info = ipa.info(&app)?;
    let executable = text(&info, "CFBundleExecutable");
    let binary = ipa.read(&format!("{app}{executable}"))?;
    let entitlements = macho::entitlement_dictionary(&binary)
        .map(|values| values.keys().cloned().collect())
        .unwrap_or_default();
    let privacy = info
        .iter()
        .filter(|(key, _)| key.ends_with(PRIVACY_SUFFIX))
        .filter_map(|(key, value)| value.as_string().map(|text| (key.clone(), text.to_owned())))
        .collect();
    Ok(IpaFacts {
        version: text(&info, "CFBundleShortVersionString"),
        build: text(&info, "CFBundleVersion"),
        minimum_os: text(&info, "MinimumOSVersion"),
        size,
        entitlements,
        privacy,
    })
}

fn merged_permissions(current: Option<&Value>, facts: &IpaFacts) -> Value {
    let mut entitlements: BTreeSet<String> = current
        .and_then(|permissions| permissions.get("entitlements"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    entitlements.extend(facts.entitlements.iter().cloned());
    let mut privacy: BTreeMap<String, String> = current
        .and_then(|permissions| permissions.get("privacy"))
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(key, value)| value.as_str().map(|text| (key.clone(), text.to_owned())))
        .collect();
    privacy.extend(facts.privacy.clone());
    json!({ "entitlements": entitlements, "privacy": privacy })
}

fn ships_phoenix(version: &str) -> bool {
    version
        .split('.')
        .next()
        .and_then(|major| major.parse::<u64>().ok())
        .is_some_and(|major| major >= FIRST_PHOENIX_MAJOR)
}

pub fn add_version(
    source: &mut Value,
    facts: &IpaFacts,
    url: &str,
    date: &str,
    description: &str,
) -> Result<(), String> {
    if !ships_phoenix(&facts.version) {
        return Err(format!(
            "Discord {} does not ship Phoenix; astral needs {FIRST_PHOENIX_MAJOR} or newer",
            facts.version
        ));
    }
    let app = source
        .get_mut("apps")
        .and_then(Value::as_array_mut)
        .and_then(|apps| apps.first_mut())
        .and_then(Value::as_object_mut)
        .ok_or("the source has no apps")?;
    let mut entry = Map::new();
    entry.insert("version".into(), json!(facts.version));
    entry.insert("buildVersion".into(), json!(facts.build));
    entry.insert("date".into(), json!(date));
    entry.insert("size".into(), json!(facts.size));
    entry.insert("downloadURL".into(), json!(url));
    entry.insert("localizedDescription".into(), json!(description));
    if !facts.minimum_os.is_empty() {
        entry.insert("minOSVersion".into(), json!(facts.minimum_os));
    }
    let permissions = merged_permissions(app.get("appPermissions"), facts);
    app.insert("appPermissions".into(), permissions);
    let versions = app.entry("versions").or_insert_with(|| json!([]));
    let list = versions.as_array_mut().ok_or("versions is not a list")?;
    list.retain(|existing| {
        existing.get("downloadURL").and_then(Value::as_str) != Some(url)
            && existing
                .get("version")
                .and_then(Value::as_str)
                .is_some_and(ships_phoenix)
    });
    list.insert(0, Value::Object(entry));
    list.truncate(MAX_VERSIONS);
    Ok(())
}

pub fn run(arguments: &[String]) -> Result<(), String> {
    let [command, rest @ ..] = arguments else {
        return Err(USAGE.into());
    };
    if command != "app-source" {
        return Err(USAGE.into());
    }
    let flags = Flags::new("feed app-source")
        .string("source", "")
        .string("ipa", "")
        .string("url", "")
        .string("date", "")
        .string("description", "")
        .parse(rest)?;
    let (source_path, ipa, url) = (flags.get("source"), flags.get("ipa"), flags.get("url"));
    if source_path.is_empty() || ipa.is_empty() || url.is_empty() {
        return Err(USAGE.into());
    }
    let (date, description) = (flags.get("date"), flags.get("description"));
    let text =
        std::fs::read_to_string(&source_path).map_err(|error| format!("{source_path}: {error}"))?;
    let mut source: Value =
        serde_json::from_str(&text).map_err(|error| format!("{source_path}: {error}"))?;
    let facts = ipa_facts(Path::new(&ipa))?;
    add_version(&mut source, &facts, &url, &date, &description)?;
    let rendered = serde_json::to_string_pretty(&source).map_err(|error| error.to_string())?;
    std::fs::write(&source_path, rendered + "\n")
        .map_err(|error| format!("{source_path}: {error}"))?;
    println!("added {} ({}) to {source_path}", facts.version, facts.build);
    Ok(())
}
