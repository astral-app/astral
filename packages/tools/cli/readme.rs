use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use regex::Regex;
use serde::Deserialize;

use crate::flags::Flags;
use crate::repo;

const ICON_SIZE: u32 = 20;
const GREY: &str = "#8C8C94";

#[derive(Deserialize, Default, Clone)]
pub struct ReadmePlugin {
    #[serde(skip)]
    pub folder: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default, rename = "iconSvg")]
    pub icon_svg: String,
}

fn plugins_directory() -> PathBuf {
    repo::path(&["packages", "addons", "plugins"])
}

fn icons_directory() -> PathBuf {
    repo::path(&["packages", "assets", "readme-icons"])
}

pub fn read_plugins() -> Result<Vec<ReadmePlugin>, String> {
    let mut plugins = Vec::new();
    for entry in std::fs::read_dir(plugins_directory())
        .map_err(|error| error.to_string())?
        .flatten()
    {
        let manifest = entry.path().join("manifest.toml");
        if !entry.path().is_dir() || !manifest.is_file() {
            continue;
        }
        let folder = entry.file_name().to_string_lossy().into_owned();
        let mut plugin: ReadmePlugin = toml::from_str(&repo::read(&manifest)?)
            .map_err(|error| format!("{folder}/manifest.toml: {error}"))?;
        plugin.folder = folder;
        plugins.push(plugin);
    }
    plugins.sort_by_key(|plugin| plugin.name.to_lowercase());
    Ok(plugins)
}

fn icon_file(plugin: &ReadmePlugin) -> String {
    if !plugin.icon_svg.is_empty() {
        return format!("{}.svg", plugin.folder);
    }
    let word = Regex::new(r"([a-z0-9])([A-Z])").expect("pattern");
    let acronym = Regex::new(r"([A-Z])([A-Z][a-z])").expect("pattern");
    let name = acronym
        .replace_all(&word.replace_all(&plugin.icon, "$1-$2"), "$1-$2")
        .into_owned();
    format!("{}.svg", name.replace('_', "-").to_lowercase())
}

fn icon(plugin: &ReadmePlugin) -> Result<String, String> {
    if !plugin.icon_svg.is_empty() {
        return Ok(format!(
            "{}\n",
            plugin.icon_svg.trim().replace("currentColor", GREY)
        ));
    }
    let png = std::fs::read(repo::path(&[
        "packages",
        "assets",
        "plugin-icons",
        &format!("{}.png", plugin.icon),
    ]))
    .map_err(|error| {
        format!(
            "plugin icon {} for {} is missing from packages/assets/plugin-icons: {error}",
            plugin.icon, plugin.folder
        )
    })?;
    Ok([
        format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="{ICON_SIZE}" height="{ICON_SIZE}" viewBox="0 0 24 24">"#),
        r#"<filter id="grey"><feColorMatrix values="0 0 0 0 0.55 0 0 0 0 0.55 0 0 0 0 0.58 0 0 0 1 0"/></filter>"#.to_owned(),
        format!(r#"<image width="24" height="24" filter="url(#grey)" href="data:image/png;base64,{}"/>"#, STANDARD.encode(png)),
        "</svg>".to_owned(),
        String::new(),
    ]
    .join("\n"))
}

fn one_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn render() -> Result<BTreeMap<PathBuf, String>, String> {
    let plugins = read_plugins()?;
    let mut files = BTreeMap::new();
    let mut lines = vec![
        "# Plugins".to_owned(),
        String::new(),
        format!(
            "{} first-party plugins for astral, each in its own folder here.",
            plugins.len()
        ),
        String::new(),
        "<!-- Generated from each manifest.toml by `astral readme`. Do not edit by hand. -->"
            .to_owned(),
        String::new(),
    ];
    for plugin in &plugins {
        let file = icon_file(plugin);
        files.insert(icons_directory().join(&file), icon(plugin)?);
        lines.push(format!(
            r#"- <img src="../../assets/readme-icons/{file}" width="{ICON_SIZE}" height="{ICON_SIZE}" align="absmiddle" alt=""> [{}]({}/): {}"#,
            one_line(&plugin.name),
            plugin.folder,
            one_line(&plugin.description)
        ));
    }
    lines.push(String::new());
    files.insert(plugins_directory().join("README.md"), lines.join("\n"));
    Ok(files)
}

fn stale_icons(files: &BTreeMap<PathBuf, String>) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(icons_directory()) else {
        return Vec::new();
    };
    let mut stale: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| !files.contains_key(path))
        .collect();
    stale.sort();
    stale
}

fn unchanged(path: &Path, contents: &str) -> bool {
    std::fs::read(path).is_ok_and(|current| current == contents.as_bytes())
}

fn write(files: &BTreeMap<PathBuf, String>) -> Result<(), String> {
    std::fs::create_dir_all(icons_directory()).map_err(|error| error.to_string())?;
    for (path, contents) in files {
        if !unchanged(path, contents) {
            std::fs::write(path, contents).map_err(|error| error.to_string())?;
        }
    }
    for path in stale_icons(files) {
        std::fs::remove_file(path).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn check(files: &BTreeMap<PathBuf, String>) -> Result<(), String> {
    let mut outdated: Vec<String> = files
        .iter()
        .filter(|(path, contents)| !unchanged(path, contents))
        .map(|(path, _)| repo::relative(path))
        .collect();
    outdated.extend(stale_icons(files).iter().map(|path| repo::relative(path)));
    if outdated.is_empty() {
        return Ok(());
    }
    outdated.sort();
    Err(format!(
        "the plugin README is out of date; run astral readme ({})",
        outdated.join(", ")
    ))
}

pub fn regenerate() -> Result<(), String> {
    write(&render()?)
}

pub fn test() -> Result<(), String> {
    check(&render()?)
}

pub fn run(arguments: &[String]) -> Result<(), String> {
    let flags = Flags::new("readme").boolean("check").parse(arguments)?;
    let files = render()?;
    if flags.flag("check") {
        check(&files)
    } else {
        write(&files)
    }
}
