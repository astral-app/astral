use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::repo::{self, strings};

pub fn copy(source: &Path, destination: &Path) -> Result<(), String> {
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    std::fs::copy(source, destination)
        .map(|_| ())
        .map_err(|error| format!("{}: {error}", source.display()))
}

pub fn glob(pattern: &Path) -> Vec<PathBuf> {
    let mut matches: Vec<PathBuf> = glob::glob(&repo::text(pattern))
        .map(|paths| paths.flatten().collect())
        .unwrap_or_default();
    matches.sort();
    matches
}

pub fn copy_glob(pattern: &Path, destination: &Path) -> Result<(), String> {
    for source in glob(pattern) {
        if source.is_file() {
            copy(
                &source,
                &destination.join(source.file_name().unwrap_or_default()),
            )?;
        }
    }
    Ok(())
}

pub fn walk(root: &Path, skip: &dyn Fn(&Path) -> bool, visit: &mut dyn FnMut(&Path)) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            if !skip(&path) {
                walk(&path, skip, visit);
            }
        } else {
            visit(&path);
        }
    }
}

pub fn unzip(archive: &Path, destination: &Path) -> Result<(), String> {
    let file =
        std::fs::File::open(archive).map_err(|error| format!("{}: {error}", archive.display()))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|error| error.to_string())?;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(|error| error.to_string())?;
        let Some(relative) = entry.enclosed_name() else {
            return Err(format!("unsafe path in IPA: {}", entry.name()));
        };
        let target = destination.join(relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&target).map_err(|error| error.to_string())?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let mode = entry.unix_mode().unwrap_or(0o644);
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        if mode & 0o170_000 == 0o120_000 {
            std::os::unix::fs::symlink(String::from_utf8_lossy(&bytes).as_ref(), &target)
                .map_err(|error| error.to_string())?;
            continue;
        }
        let mut created = std::fs::File::create(&target).map_err(|error| error.to_string())?;
        created
            .write_all(&bytes)
            .map_err(|error| error.to_string())?;
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            &target,
            std::fs::Permissions::from_mode((mode & 0o7777) | 0o600),
        )
        .map_err(|error| error.to_string())?;
    }
    Ok(())
}

pub fn sign_bundle(app: &Path, identity: &str, entitlements: &Path) -> Result<(), String> {
    let mut nested = Vec::new();
    let frameworks = app.join("Frameworks");
    if let Ok(entries) = std::fs::read_dir(&frameworks) {
        let mut paths: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
        paths.sort();
        for path in paths {
            let name = repo::text(&path);
            if (path.is_dir() && name.ends_with(".framework"))
                || (!path.is_dir() && name.ends_with(".dylib"))
            {
                nested.push(path);
            }
        }
    }
    for item in nested {
        repo::run(
            app,
            "codesign",
            &strings(&[
                "--force",
                "--sign",
                identity,
                "--timestamp=none",
                &repo::text(&item),
            ]),
        )?;
    }
    repo::run(
        app,
        "codesign",
        &strings(&[
            "--force",
            "--sign",
            identity,
            "--timestamp=none",
            "--entitlements",
            &repo::text(entitlements),
            &repo::text(app),
        ]),
    )
}

pub fn newest(pattern: &Path) -> Option<PathBuf> {
    glob(pattern).into_iter().max_by_key(|path| {
        std::fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .ok()
    })
}
