use std::collections::BTreeMap;

use serde::Deserialize;

use crate::readme;
use crate::repo;

#[derive(Deserialize, Default, Clone)]
struct Listing {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    icon: String,
}

#[derive(Deserialize, Default)]
struct AstralSection {
    #[serde(default)]
    version: String,
}

#[derive(Deserialize, Default)]
struct Index {
    #[serde(default)]
    astral: AstralSection,
    #[serde(default)]
    plugins: Vec<Listing>,
}

fn drift() -> Result<Vec<String>, String> {
    let index: Index = toml::from_str(&repo::read(&repo::path(&[
        "packages",
        "catalog",
        "index.toml",
    ]))?)
    .map_err(|error| format!("packages/catalog/index.toml: {error}"))?;
    let version = repo::read(&repo::path(&["version.txt"]))?.trim().to_owned();
    let mut drift = Vec::new();
    if version != index.astral.version {
        drift.push(format!(
            "astral version: catalog {:?}, version.txt {version:?}",
            index.astral.version
        ));
    }
    let listed: BTreeMap<String, Listing> = index
        .plugins
        .into_iter()
        .map(|plugin| (plugin.id.clone(), plugin))
        .collect();
    for plugin in readme::read_plugins()? {
        let manifest: Listing = toml::from_str(&repo::read(&repo::path(&[
            "packages",
            "addons",
            "plugins",
            &plugin.folder,
            "manifest.toml",
        ]))?)
        .map_err(|error| error.to_string())?;
        let Some(entry) = listed.get(&manifest.id) else {
            continue;
        };
        let want_icon = if manifest.icon.ends_with("Icon") {
            manifest.icon.clone()
        } else {
            String::new()
        };
        for (field, catalog, wanted) in [
            ("name", &entry.name, &manifest.name),
            ("version", &entry.version, &manifest.version),
            ("description", &entry.description, &manifest.description),
            ("icon", &entry.icon, &want_icon),
        ] {
            if catalog != wanted {
                drift.push(format!(
                    "{} {field}: catalog {catalog:?}, manifest {wanted:?}",
                    manifest.id
                ));
            }
        }
    }
    Ok(drift)
}

pub fn test() -> Result<(), String> {
    let drift = drift()?;
    if drift.is_empty() {
        return Ok(());
    }
    Err(format!(
        "packages/catalog/index.toml disagrees with the plugin manifests:\n  {}",
        drift.join("\n  ")
    ))
}
