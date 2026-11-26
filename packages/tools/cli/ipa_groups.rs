use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::files;
use crate::ipa_archive::{self as archive, Action, Addition, Ipa};
use crate::macho;
use crate::repo::{self, strings};

pub const GROUP_REMAP_FRAMEWORK: &str = "Frameworks/AstralGroups.dylib";
pub const GROUP_REMAP_INSTALL: &str = "@rpath/AstralGroups.dylib";
pub const OPEN_IN_ASTRAL: &str = "OpenInAstral.appex";
pub const REQUIRED_EXTENSIONS: [&str; 2] = [OPEN_IN_ASTRAL, "Share.appex"];
const MANIFEST: &str = "AstralExtensions.json";

pub fn is_own_extension(name: &str) -> bool {
    name == OPEN_IN_ASTRAL
}

#[derive(Clone)]
pub struct Extension {
    pub name: String,
    pub executable: String,
    pub bundle_id: String,
}

pub fn build_group_remap(work: &Path) -> Result<PathBuf, String> {
    let source = repo::path(&["packages", "ios", "groups", "AstralGroups.m"]);
    let output = work.join("AstralGroups.dylib");
    repo::run(
        work,
        "xcrun",
        &strings(&[
            "-sdk",
            "iphoneos",
            "clang",
            "-dynamiclib",
            "-arch",
            "arm64",
            "-miphoneos-version-min=18.0",
            "-fobjc-arc",
            "-framework",
            "Foundation",
            "-install_name",
            GROUP_REMAP_INSTALL,
            "-o",
            &repo::text(&output),
            &repo::text(&source),
        ]),
    )
    .map_err(|error| format!("building AstralGroups.dylib failed: {error}"))?;
    Ok(output)
}

pub fn validate_group_remap_library(data: &[u8]) -> Result<(), String> {
    let file =
        macho::parse(data).map_err(|error| format!("AstralGroups.dylib is not Mach-O: {error}"))?;
    if !file.is_64 || file.cpu != macho::CPU_ARM64 || file.file_type != macho::TYPE_DYLIB {
        return Err("AstralGroups.dylib must be an arm64 dylib".into());
    }
    if !file.loads_dylib(GROUP_REMAP_INSTALL) {
        return Err(format!(
            "AstralGroups.dylib has no install name {GROUP_REMAP_INSTALL}"
        ));
    }
    Ok(())
}

fn valid_executable(name: &str) -> bool {
    !name.is_empty() && !name.contains('/') && name != "." && name != ".."
}

pub fn extensions(ipa: &mut Ipa, app: &str) -> Result<Vec<Extension>, String> {
    let prefix = format!("{app}PlugIns/");
    let mut paths: Vec<String> = ipa
        .names()
        .into_iter()
        .filter(|name| {
            name.strip_prefix(&prefix).is_some_and(|rest| {
                let parts: Vec<&str> = rest.split('/').collect();
                parts.len() == 2 && parts[1] == "Info.plist" && parts[0].ends_with(".appex")
            })
        })
        .collect();
    paths.sort();
    let mut found = Vec::new();
    for path in paths {
        let info = archive::read_plist(&ipa.read(&path)?)?;
        let name = path[prefix.len()..]
            .split('/')
            .next()
            .unwrap_or_default()
            .to_owned();
        let executable = archive::text(&info, "CFBundleExecutable");
        if !valid_executable(&executable) {
            return Err(format!(
                "extension has an invalid CFBundleExecutable: {name}"
            ));
        }
        found.push(Extension {
            executable: format!("{prefix}{name}/{executable}"),
            bundle_id: archive::text(&info, "CFBundleIdentifier"),
            name,
        });
    }
    Ok(found)
}

pub fn add_group_remap(ipa_path: &Path, library: &Path) -> Result<(), String> {
    let dylib = std::fs::read(library).map_err(|error| error.to_string())?;
    validate_group_remap_library(&dylib)?;
    let mut ipa = Ipa::open(ipa_path)?;
    let app = ipa.app()?;
    let patched: BTreeMap<String, String> = extensions(&mut ipa, &app)?
        .into_iter()
        .filter(|extension| !is_own_extension(&extension.name))
        .map(|extension| (extension.executable, extension.name))
        .collect();
    drop(ipa);
    let framework = format!("{app}{GROUP_REMAP_FRAMEWORK}");
    archive::rewrite(
        ipa_path,
        "groups",
        |name| {
            if name == framework {
                return Action::Drop;
            }
            match patched.get(name) {
                Some(extension) => {
                    let extension = extension.clone();
                    Action::Patch(Box::new(move |data| {
                        macho::add_weak_dylib(&data, GROUP_REMAP_INSTALL).map_err(|error| {
                            format!("wiring AstralGroups.dylib into {extension} failed: {error}")
                        })
                    }))
                }
                None => Action::Keep,
            }
        },
        vec![Addition {
            name: framework.clone(),
            mode: 0o755,
            data: dylib,
        }],
    )
}

fn check_extension_groups(name: &str, data: &[u8], app_groups: &[String]) -> Result<(), String> {
    if !macho::loads_dylib(data, GROUP_REMAP_INSTALL) {
        return Err(format!("{name} does not load AstralGroups.dylib"));
    }
    for group in macho::app_groups(data) {
        if !app_groups.contains(&group) {
            return Err(format!(
                "{name} uses app group {group}, which the app does not"
            ));
        }
    }
    Ok(())
}

fn require_found(found_share: bool, found_astral: bool, subject: &str) -> Result<(), String> {
    if !found_share {
        return Err(format!("{subject} has no Discord Share.appex"));
    }
    if !found_astral {
        return Err(format!("{subject} has no OpenInAstral.appex"));
    }
    Ok(())
}

fn valid_manifest(data: &[u8]) -> bool {
    serde_json::from_slice::<serde_json::Value>(data)
        .ok()
        .and_then(|value| {
            value
                .get("manifest_version")
                .and_then(serde_json::Value::as_i64)
        })
        == Some(3)
}

pub fn validate_ipa_group_wiring(ipa_path: &Path) -> Result<(), String> {
    let mut ipa = Ipa::open(ipa_path)?;
    let app = ipa.app()?;
    let found = extensions(&mut ipa, &app)?;
    let library = ipa
        .read(&format!("{app}{GROUP_REMAP_FRAMEWORK}"))
        .map_err(|_| format!("final IPA has no {GROUP_REMAP_FRAMEWORK}"))?;
    validate_group_remap_library(&library)?;
    let info = ipa.info(&app)?;
    let app_id = archive::text(&info, "CFBundleIdentifier");
    if app_id.is_empty() {
        return Err("the app has no CFBundleIdentifier".into());
    }
    let executable = archive::text(&info, "CFBundleExecutable");
    let executable_path = format!("{app}{executable}");
    let app_groups = if !executable.is_empty() && ipa.has(&executable_path) {
        macho::app_groups(&ipa.read(&executable_path)?)
    } else {
        Vec::new()
    };
    let (mut found_share, mut found_astral) = (false, false);
    for extension in found {
        if !extension.bundle_id.starts_with(&format!("{app_id}.")) {
            return Err(format!(
                "{} has bundle identifier {:?} outside {app_id}",
                extension.name, extension.bundle_id
            ));
        }
        if is_own_extension(&extension.name) {
            found_astral = true;
            let manifest = ipa
                .read(&format!("{app}PlugIns/{}/manifest.json", extension.name))
                .map_err(|_| format!("{} has no Safari Web Extension manifest", extension.name))?;
            if !valid_manifest(&manifest) {
                return Err(format!(
                    "{} has an invalid Safari Web Extension manifest",
                    extension.name
                ));
            }
            continue;
        }
        if extension.name == "Share.appex" {
            found_share = true;
        }
        let data = ipa.read(&extension.executable).map_err(|_| {
            format!(
                "{} has no executable at {}",
                extension.name, extension.executable
            )
        })?;
        check_extension_groups(&extension.name, &data, &app_groups)?;
    }
    require_found(found_share, found_astral, "final IPA")
}

pub fn extension_bundles(app: &Path) -> Result<Vec<PathBuf>, String> {
    let entries = match std::fs::read_dir(app.join("PlugIns")) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.to_string()),
    };
    let mut bundles: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && repo::text(path).ends_with(".appex"))
        .collect();
    bundles.sort();
    Ok(bundles)
}

fn base_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

pub fn validate_signed_group_wiring(app: &Path) -> Result<(), String> {
    let info = archive::read_plist_file(&app.join("Info.plist"))?;
    let app_id = archive::text(&info, "CFBundleIdentifier");
    let executable = archive::text(&info, "CFBundleExecutable");
    if app_id.is_empty() || !valid_executable(&executable) {
        return Err("signed app has invalid bundle identity".into());
    }
    let app_binary = std::fs::read(app.join(&executable)).map_err(|error| error.to_string())?;
    let app_groups = macho::app_groups(&app_binary);
    let library = std::fs::read(app.join(GROUP_REMAP_FRAMEWORK))
        .map_err(|error| format!("signed app has no {GROUP_REMAP_FRAMEWORK}: {error}"))?;
    validate_group_remap_library(&library)?;
    let (mut found_share, mut found_astral) = (false, false);
    for bundle in extension_bundles(app)? {
        let name = base_name(&bundle);
        let info = archive::read_plist_file(&bundle.join("Info.plist"))
            .map_err(|error| format!("{name} has no readable Info.plist: {error}"))?;
        let identifier = archive::text(&info, "CFBundleIdentifier");
        if !identifier.starts_with(&format!("{app_id}.")) {
            return Err(format!(
                "{name} has bundle identifier {identifier:?} outside {app_id}"
            ));
        }
        if name == "Share.appex" {
            found_share = true;
        }
        if is_own_extension(&name) {
            found_astral = true;
            continue;
        }
        let executable = archive::text(&info, "CFBundleExecutable");
        if !valid_executable(&executable) {
            return Err(format!("{name} has an invalid executable name"));
        }
        let data = std::fs::read(bundle.join(&executable))
            .map_err(|error| format!("{name} has no readable executable: {error}"))?;
        check_extension_groups(&name, &data, &app_groups)?;
    }
    require_found(found_share, found_astral, "signed app")
}

pub fn extension_names(entries: &[String]) -> BTreeSet<String> {
    entries
        .iter()
        .filter_map(|entry| {
            let parts: Vec<&str> = entry.split('/').collect();
            let ["Payload", app, "PlugIns", appex, "Info.plist"] = parts.as_slice() else {
                return None;
            };
            (app.ends_with(".app") && appex.ends_with(".appex")).then(|| (*appex).to_owned())
        })
        .collect()
}

pub fn validate_ipa_extensions(
    source: &[String],
    last: &[String],
    required: &[&str],
) -> Result<(), String> {
    let final_names = extension_names(last);
    for extension in extension_names(source) {
        if !final_names.contains(&extension) {
            return Err(format!(
                "IPA build dropped the Discord extension {extension}"
            ));
        }
    }
    for extension in required {
        if !final_names.contains(*extension) {
            return Err(format!("IPA build omitted astral's extension {extension}"));
        }
    }
    Ok(())
}

pub fn valid_extension_name(name: &str) -> bool {
    name.ends_with(".appex") && !name.contains('/') && name != ".appex"
}

pub fn extension_manifest(source: &[String], required: &[&str]) -> Result<Vec<u8>, String> {
    let mut expected = extension_names(source);
    for extension in required {
        if !valid_extension_name(extension) {
            return Err(format!("invalid required extension name: {extension}"));
        }
        expected.insert((*extension).to_owned());
    }
    if expected.is_empty() {
        return Err("the source IPA contains no app extensions".into());
    }
    serde_json::to_vec(&serde_json::json!({ "extensions": expected }))
        .map_err(|error| error.to_string())
}

pub fn add_extension_manifest(
    ipa_path: &Path,
    source: &[String],
    required: &[&str],
) -> Result<(), String> {
    let manifest = extension_manifest(source, required)?;
    let app = Ipa::open(ipa_path)?.app()?;
    let path = format!("{app}{MANIFEST}");
    archive::rewrite(
        ipa_path,
        "extension-manifest",
        |name| {
            if name == path {
                Action::Drop
            } else {
                Action::Keep
            }
        },
        vec![Addition {
            name: path.clone(),
            mode: 0o644,
            data: manifest,
        }],
    )
}

pub fn validate_signed_extensions(app: &Path) -> Result<(), String> {
    let data = std::fs::read(app.join(MANIFEST))
        .map_err(|error| format!("signed app extension manifest is unavailable: {error}"))?;
    let listed: Vec<String> = serde_json::from_slice::<serde_json::Value>(&data)
        .ok()
        .and_then(|value| value.get("extensions").cloned())
        .and_then(|value| serde_json::from_value(value).ok())
        .filter(|listed: &Vec<String>| !listed.is_empty())
        .ok_or("signed app extension manifest is invalid")?;
    let mut expected = BTreeSet::new();
    for extension in listed {
        if !valid_extension_name(&extension) || !expected.insert(extension) {
            return Err("signed app extension manifest contains an invalid entry".into());
        }
    }
    for bundle in extension_bundles(app)? {
        expected.remove(&base_name(&bundle));
    }
    if expected.is_empty() {
        return Ok(());
    }
    Err(format!(
        "signed app is missing required extensions: {}",
        expected.into_iter().collect::<Vec<_>>().join(", ")
    ))
}

pub fn add_open_in_astral(ipa_path: &Path, extension: &Path) -> Result<(), String> {
    let app = Ipa::open(ipa_path)?.app()?;
    let prefix = format!("{app}PlugIns/{OPEN_IN_ASTRAL}/");
    let mut additions = Vec::new();
    let mut failure = None;
    files::walk(extension, &|_| false, &mut |path| {
        if failure.is_some() || path.is_dir() {
            return;
        }
        let metadata = match std::fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) => return failure = Some(error.to_string()),
        };
        if !metadata.file_type().is_file() {
            return failure = Some(format!(
                "OpenInAstral.appex contains a non-regular file: {}",
                path.display()
            ));
        }
        let relative = path
            .strip_prefix(extension)
            .map(repo::text)
            .unwrap_or_default();
        match std::fs::read(path) {
            Ok(data) => additions.push(Addition {
                name: format!("{prefix}{relative}"),
                mode: std::os::unix::fs::PermissionsExt::mode(&metadata.permissions()),
                data,
            }),
            Err(error) => failure = Some(error.to_string()),
        }
    });
    if let Some(error) = failure {
        return Err(error);
    }
    archive::rewrite(
        ipa_path,
        "extension",
        |name| {
            if name.starts_with(&prefix) {
                Action::Drop
            } else {
                Action::Keep
            }
        },
        additions,
    )
    .map_err(|error| format!("installing OpenInAstral.appex in the IPA failed: {error}"))
}
