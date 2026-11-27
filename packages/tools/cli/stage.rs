use std::path::Path;

use crate::files;
use crate::repo;
use crate::signing;

fn stage_plugin(folder: &Path, destination: &Path) -> Result<(), String> {
    let mut paths = signing::signed_paths(folder)?;
    paths.push("signature".into());
    for path in paths {
        files::copy(&folder.join(&path), &destination.join(&path))?;
    }
    Ok(())
}

pub fn stage_catalog(target: &Path) -> Result<(), String> {
    if target.exists() {
        std::fs::remove_dir_all(target).map_err(|error| error.to_string())?;
    }
    for folder in repo::plugin_folders()? {
        let manifest = signing::read_manifest(&folder)?;
        stage_plugin(&folder, &target.join("plugins").join(&manifest.id))?;
    }
    files::copy_glob(&repo::path(&["packages", "catalog", "index.toml*"]), target)?;
    files::copy_glob(
        &repo::path(&["packages", "catalog", "themes", "*.toml"]),
        &target.join("themes"),
    )?;
    Ok(())
}

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
    match arguments {
        [flag, target] if flag == "--catalog" => stage_catalog(Path::new(target)),
        [target] => stage_resources(Path::new(target)),
        _ => Err("usage: astral stage [--catalog] <directory>".into()),
    }
}
