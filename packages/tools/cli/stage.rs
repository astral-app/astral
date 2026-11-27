use std::path::Path;

use crate::files;
use crate::repo;

pub fn stage_resources(target: &Path) -> Result<(), String> {
    if target.exists() {
        std::fs::remove_dir_all(target).map_err(|error| error.to_string())?;
    }
    let mut copies = vec![
        (
            repo::path(&["packages", "addons", "themes", "*.toml"]),
            target.join("themes"),
        ),
        (
            repo::path(&["packages", "addons", "themes", "palettes", "*.toml"]),
            target.join("palettes"),
        ),
        (
            repo::path(&["packages", "catalog", "index.toml*"]),
            target.join("catalog"),
        ),
        (
            repo::path(&["packages", "catalog", "themes", "*.toml"]),
            target.join("catalog/themes"),
        ),
        (
            repo::path(&["packages", "assets", "*.svg"]),
            target.join("assets"),
        ),
    ];
    for pack in files::glob(&repo::path(&["packages", "addons", "emoji-packs", "*"])) {
        let name = pack.file_name().unwrap_or_default().to_owned();
        copies.push((pack.join("*"), target.join("emoji-packs").join(name)));
    }
    for pack in files::glob(&repo::path(&["packages", "addons", "icon-packs", "*"])) {
        let name = pack.file_name().unwrap_or_default().to_owned();
        copies.push((
            pack.join("manifest.toml"),
            target.join("icon-packs").join(&name),
        ));
        copies.push((
            pack.join("icons/*.png"),
            target.join("icon-packs").join(&name).join("icons"),
        ));
    }
    for (pattern, destination) in copies {
        files::copy_glob(&pattern, &destination)?;
    }
    Ok(())
}

pub fn run(arguments: &[String]) -> Result<(), String> {
    let [target] = arguments else {
        return Err("usage: astral stage <directory>".into());
    };
    stage_resources(Path::new(target))
}
