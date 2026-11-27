use std::path::{Path, PathBuf};
use std::time::SystemTime;

use plist::{Dictionary, Value};
use sha1::{Digest, Sha1};

use crate::files;
use crate::flags::{Flags, environment};
use crate::ipa;
use crate::ipa_archive::{self as archive};
use crate::ipa_groups::{self as groups, REQUIRED_EXTENSIONS};
use crate::repo::{self, strings};

const USAGE: &str = "usage: astral sideload --ipa <IPA> --bundle-id <id> [--device <udid>] [--profile <mobileprovision>]";

#[derive(Clone, Default)]
pub struct Profile {
    pub path: PathBuf,
    pub name: String,
    pub expiration: Option<SystemTime>,
    pub devices: Vec<String>,
    pub certificates: Vec<Vec<u8>>,
    pub entitlements: Dictionary,
    pub team: Vec<String>,
}

fn texts(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_string)
                .filter(|item| !item.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

impl Profile {
    pub fn from_plist(path: PathBuf, values: &Dictionary) -> Profile {
        Profile {
            path,
            name: archive::text(values, "Name"),
            expiration: values
                .get("ExpirationDate")
                .and_then(Value::as_date)
                .map(SystemTime::from),
            devices: texts(values.get("ProvisionedDevices")),
            certificates: values
                .get("DeveloperCertificates")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_data)
                        .map(<[u8]>::to_vec)
                        .collect()
                })
                .unwrap_or_default(),
            entitlements: values
                .get("Entitlements")
                .and_then(Value::as_dictionary)
                .cloned()
                .unwrap_or_default(),
            team: texts(values.get("TeamIdentifier")),
        }
    }

    pub fn application_identifier(&self) -> String {
        archive::text(&self.entitlements, "application-identifier")
    }

    pub fn application_groups(&self) -> Vec<String> {
        texts(
            self.entitlements
                .get("com.apple.security.application-groups"),
        )
    }

    fn first_application_group(&self) -> String {
        self.application_groups()
            .into_iter()
            .next()
            .unwrap_or_default()
    }

    pub fn covers(&self, bundle_id: &str, device: &str) -> bool {
        let Some(team) = self.team.first() else {
            return false;
        };
        if self
            .expiration
            .is_none_or(|expiration| SystemTime::now() > expiration)
        {
            return false;
        }
        let identifier = self.application_identifier();
        let pattern = identifier
            .strip_prefix(&format!("{team}."))
            .unwrap_or(&identifier);
        let matches = pattern == bundle_id
            || pattern
                .strip_suffix('*')
                .is_some_and(|prefix| bundle_id.starts_with(prefix));
        matches
            && self
                .devices
                .iter()
                .any(|provisioned| provisioned.eq_ignore_ascii_case(device))
    }

    fn team_id(&self) -> &str {
        self.team.first().map_or("", String::as_str)
    }
}

pub fn read_profile(path: &Path) -> Result<Profile, String> {
    let decoded = repo::output(
        &repo::path(&[]),
        "security",
        &strings(&["cms", "-D", "-i", &repo::text(path)]),
    )?;
    Ok(Profile::from_plist(
        path.to_owned(),
        &archive::read_plist(decoded.as_bytes())?,
    ))
}

fn find_profile(bundle_id: &str, device: &str) -> Result<Profile, String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let mut matching: Vec<Profile> = [
        "Library/Developer/Xcode/UserData/Provisioning Profiles",
        "Library/MobileDevice/Provisioning Profiles",
    ]
    .iter()
    .flat_map(|folder| files::glob(&Path::new(&home).join(folder).join("*.mobileprovision")))
    .filter_map(|candidate| read_profile(&candidate).ok())
    .filter(|profile| profile.covers(bundle_id, device))
    .collect();
    if matching.is_empty() {
        return Err(format!(
            "no provisioning profile covers {bundle_id} on {device}; pass --profile"
        ));
    }
    matching.sort_by_key(|profile| profile.application_identifier().contains('*'));
    Ok(matching.swap_remove(0))
}

fn signing_identity(profile: &Profile) -> Result<String, String> {
    let listing = repo::output(
        &repo::path(&[]),
        "security",
        &strings(&["find-identity", "-v", "-p", "codesigning"]),
    )?;
    let pattern = regex::Regex::new(r#"\) ([0-9A-F]{40}) ""#).map_err(|error| error.to_string())?;
    let available: Vec<String> = listing
        .lines()
        .filter(|line| !line.contains("CSSMERR"))
        .filter_map(|line| pattern.captures(line).map(|found| found[1].to_owned()))
        .collect();
    profile
        .certificates
        .iter()
        .map(|certificate| {
            Sha1::digest(certificate)
                .iter()
                .map(|byte| format!("{byte:02X}"))
                .collect::<String>()
        })
        .find(|hash| available.contains(hash))
        .ok_or_else(|| {
            format!(
                "no signing identity in the keychain matches {}",
                profile.name
            )
        })
}

pub fn entitlements_for(
    profile: &Profile,
    bundle_id: &str,
    directory: &Path,
) -> Result<PathBuf, String> {
    let mut entitlements = profile.entitlements.clone();
    let team = profile.team_id();
    let identifier = format!("{team}.{bundle_id}");
    entitlements.insert(
        "application-identifier".into(),
        Value::String(identifier.clone()),
    );
    if let Some(Value::Array(groups)) = entitlements.get_mut("keychain-access-groups") {
        for group in groups.iter_mut() {
            if group.as_string().is_some_and(|text| text.ends_with('*')) {
                *group = Value::String(identifier.clone());
            }
        }
    }
    let path = directory.join("entitlements.plist");
    std::fs::write(&path, archive::write_plist(entitlements, false)?)
        .map_err(|error| error.to_string())?;
    Ok(path)
}

fn edit_info(bundle: &Path, edit: impl FnOnce(&mut Dictionary)) -> Result<(), String> {
    let path = bundle.join("Info.plist");
    let data = std::fs::read(&path).map_err(|error| error.to_string())?;
    let mut values = archive::read_plist(&data)?;
    edit(&mut values);
    std::fs::write(
        &path,
        archive::write_plist(values, archive::is_binary_plist(&data))?,
    )
    .map_err(|error| error.to_string())
}

fn set_app_bundle_id(app: &Path, bundle_id: &str) -> Result<(), String> {
    edit_info(app, |values| {
        values.insert("CFBundleIdentifier".into(), Value::String(bundle_id.into()));
        archive::ensure_url_scheme(values, ipa::URL_SCHEME);
        values.remove("UISupportedDevices");
    })
}

fn bundle_identifier(bundle: &Path) -> Result<String, String> {
    let identifier = archive::text(
        &archive::read_plist_file(&bundle.join("Info.plist"))?,
        "CFBundleIdentifier",
    );
    if identifier.is_empty() {
        return Err(format!(
            "bundle has no CFBundleIdentifier: {}",
            bundle.display()
        ));
    }
    Ok(identifier)
}

pub fn extension_bundle_id(
    original_app_id: &str,
    target_app_id: &str,
    extension_id: &str,
) -> Result<String, String> {
    match extension_id.strip_prefix(original_app_id) {
        Some(suffix) if suffix.starts_with('.') => Ok(format!("{target_app_id}{suffix}")),
        _ => Err(format!(
            "extension bundle identifier {extension_id:?} is not nested under {original_app_id:?}"
        )),
    }
}

fn base_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn check_extension_profile(
    profile: &Profile,
    extension: &Profile,
    target: &str,
    own: bool,
) -> Result<(), String> {
    if profile.team.is_empty()
        || extension.team.is_empty()
        || profile.team_id() != extension.team_id()
    {
        return Err(format!(
            "provisioning profile for extension {target} must use the app's team"
        ));
    }
    if own {
        return Ok(());
    }
    let app_group = profile.first_application_group();
    if extension.first_application_group() != app_group {
        return Err(format!(
            "provisioning profile for extension {target} must use the app's application group {app_group:?}"
        ));
    }
    let app_groups = profile.application_groups();
    for group in extension.application_groups() {
        if !app_groups.contains(&group) {
            return Err(format!(
                "provisioning profile for extension {target} adds app group {group:?} that the app does not have"
            ));
        }
    }
    Ok(())
}

struct Signing<'a> {
    app: &'a Path,
    original_app_id: &'a str,
    bundle_id: &'a str,
    profile: &'a Profile,
    device: &'a str,
    work: &'a Path,
}

fn sign_extensions(signing: &Signing) -> Result<(), String> {
    let bundles = groups::extension_bundles(signing.app)?;
    let needs_group = bundles
        .iter()
        .any(|bundle| !groups::is_own_extension(&base_name(bundle)));
    if needs_group && signing.profile.first_application_group().is_empty() {
        return Err("the app provisioning profile has no application group entitlement for Discord's extensions".into());
    }
    for bundle in bundles {
        let name = base_name(&bundle);
        let target = extension_bundle_id(
            signing.original_app_id,
            signing.bundle_id,
            &bundle_identifier(&bundle)?,
        )?;
        let profile = if signing.profile.covers(&target, signing.device) {
            signing.profile.clone()
        } else {
            find_profile(&target, signing.device)?
        };
        check_extension_profile(
            signing.profile,
            &profile,
            &target,
            groups::is_own_extension(&name),
        )?;
        let identity = signing_identity(&profile)?;
        edit_info(&bundle, |values| {
            values.insert("CFBundleIdentifier".into(), Value::String(target.clone()));
        })?;
        files::copy(&profile.path, &bundle.join("embedded.mobileprovision"))?;
        let directory = signing.work.join(name.trim_end_matches(".appex"));
        std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        let entitlements = entitlements_for(&profile, &target, &directory)?;
        files::sign_bundle(&bundle, &identity, &entitlements)?;
    }
    Ok(())
}

fn wire(
    source: &Path,
    injected: bool,
    source_entries: &[String],
    work: &Path,
) -> Result<PathBuf, String> {
    let remap = groups::build_group_remap(work)?;
    let wired = work.join("wired.ipa");
    files::copy(source, &wired)?;
    groups::add_extension_manifest(&wired, source_entries, &REQUIRED_EXTENSIONS)
        .map_err(|error| format!("recording signed app extension requirements failed: {error}"))?;
    if injected {
        let extension = ipa::build_open_in_astral(work, &ipa::host_app(&wired)?)?;
        groups::add_open_in_astral(&wired, &extension)?;
    }
    groups::add_group_remap(&wired, &remap)?;
    groups::validate_ipa_group_wiring(&wired)?;
    Ok(wired)
}

pub fn run(arguments: &[String]) -> Result<(), String> {
    let flags = Flags::new("sideload")
        .string("device", &environment("DEVICE"))
        .string("ipa", "")
        .string("bundle-id", "")
        .string("profile", "")
        .parse(arguments)?;
    let (device, bundle_id, profile_path) = (
        flags.get("device"),
        flags.get("bundle-id"),
        flags.get("profile"),
    );
    let ipa_path = PathBuf::from(flags.get("ipa"));
    if ipa_path.as_os_str().is_empty() || bundle_id.is_empty() {
        return Err(USAGE.into());
    }
    if device.is_empty() {
        return Err("set --device or DEVICE to the test device UDID".into());
    }
    let profile = if profile_path.is_empty() {
        find_profile(&bundle_id, &device)?
    } else {
        read_profile(Path::new(&profile_path))?
    };
    if !profile.covers(&bundle_id, &device) {
        return Err(format!(
            "provisioning profile {} does not cover {bundle_id} on {device}",
            profile.name
        ));
    }
    let identity = signing_identity(&profile)?;
    let work = repo::scratch()?;
    let injected = ipa::loads_astral(&ipa_path)?;
    let source_entries = archive::entries(&ipa_path)?;
    let source = if injected {
        ipa_path
    } else {
        let built = work.0.join("astral.ipa");
        ipa::build(
            &std::path::absolute(&ipa_path).map_err(|error| error.to_string())?,
            &crate::device::newest_deb()?,
            &built,
        )?;
        built
    };
    let wired = wire(&source, injected, &source_entries, &work.0)?;
    let extracted = work.0.join("extracted");
    files::unzip(&wired, &extracted)?;
    let apps = files::glob(&extracted.join("Payload/*.app"));
    let [app] = apps.as_slice() else {
        return Err("the IPA has no single Payload/*.app".into());
    };
    for folder in ["Watch", "Extensions"] {
        let _ = std::fs::remove_dir_all(app.join(folder));
    }
    let original_app_id = bundle_identifier(app)?;
    set_app_bundle_id(app, &bundle_id)?;
    sign_extensions(&Signing {
        app,
        original_app_id: &original_app_id,
        bundle_id: &bundle_id,
        profile: &profile,
        device: &device,
        work: &work.0,
    })?;
    files::copy(&profile.path, &app.join("embedded.mobileprovision"))?;
    let entitlements = entitlements_for(&profile, &bundle_id, &work.0)?;
    files::sign_bundle(app, &identity, &entitlements)?;
    groups::validate_signed_extensions(app)?;
    groups::validate_signed_group_wiring(app)?;
    repo::run(
        &work.0,
        "xcrun",
        &strings(&[
            "devicectl",
            "device",
            "install",
            "app",
            "--device",
            &device,
            &repo::text(app),
        ]),
    )?;
    println!(
        "installed {bundle_id} on {device}, signed with {}",
        profile.name
    );
    Ok(())
}
