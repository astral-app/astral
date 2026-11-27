use std::path::Path;

use crate::files;
use crate::repo;
use crate::signing;

fn stage_signed(mut paths: Vec<String>, folder: &Path, destination: &Path) -> Result<(), String> {
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
        let destination = target.join("plugins").join(&manifest.id);
        stage_signed(signing::signed_paths(&folder)?, &folder, &destination)?;
    }
    for folder in repo::pack_folders()? {
        let manifest = signing::read_manifest(&folder)?;
        let kind = folder.parent().map(signing::name_of).unwrap_or_default();
        let destination = target.join(kind).join(&manifest.id);
        stage_signed(signing::pack_paths(&folder)?, &folder, &destination)?;
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
    let copies = vec![
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
        (
            repo::path(&["packages", "assets", "*.png"]),
            target.join("assets"),
        ),
    ];
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
