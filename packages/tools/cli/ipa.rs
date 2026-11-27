use std::path::{Path, PathBuf};

use crate::device;
use crate::files;
use crate::flags::Flags;
use crate::ipa_archive::{self as archive, Action, Ipa};
use crate::ipa_groups::{self as groups, REQUIRED_EXTENSIONS};
use crate::macho;
use crate::repo::{self, strings};
use crate::stage;

const CYAN_REPOSITORY: &str = "https://github.com/asdfzxcvbn/pyzule-rw";
const CYAN_COMMIT: &str = "740d3716dcd98c20c000f12cdb88f1f0b2a533a4";
const PILLOW_VERSION: &str = "11.3.0";
pub const URL_SCHEME: &str = "astral";
const USAGE: &str = "usage: astral build-ipa --ipa <decrypted Discord.ipa> [--deb <astral.deb>] [--output <astral.ipa>]";

fn cyan_binary() -> Result<PathBuf, String> {
    let cache = repo::path(&[".tools-cache", &format!("cyan-{CYAN_COMMIT}")]);
    let venv = cache.join("venv");
    let binary = venv.join("bin/cyan");
    if binary.exists() {
        return Ok(binary);
    }
    std::fs::create_dir_all(&cache).map_err(|error| error.to_string())?;
    repo::run(
        &cache,
        "python3",
        &strings(&["-m", "venv", &repo::text(&venv)]),
    )?;
    repo::run(
        &cache,
        &repo::text(&venv.join("bin/python3")),
        &strings(&[
            "-m",
            "pip",
            "install",
            "--disable-pip-version-check",
            "--quiet",
            &format!("{CYAN_REPOSITORY}/archive/{CYAN_COMMIT}.zip"),
            &format!("Pillow=={PILLOW_VERSION}"),
        ]),
    )?;
    Ok(binary)
}

pub fn loads_astral(ipa_path: &Path) -> Result<bool, String> {
    let mut ipa = Ipa::open(ipa_path)?;
    let app = ipa.app()?;
    let info = ipa.info(&app)?;
    let executable = ipa.read(&format!(
        "{app}{}",
        archive::text(&info, "CFBundleExecutable")
    ))?;
    let file = macho::parse(&executable)?;
    Ok(file
        .loaded_dylibs()
        .iter()
        .any(|library| library.ends_with("/astral.dylib")))
}

pub struct HostApp {
    pub identifier: String,
    pub version: String,
    pub build: String,
}

pub fn host_app(ipa_path: &Path) -> Result<HostApp, String> {
    let mut ipa = Ipa::open(ipa_path)?;
    let app = ipa.app()?;
    let info = ipa.info(&app)?;
    let host = HostApp {
        identifier: archive::text(&info, "CFBundleIdentifier"),
        version: archive::text(&info, "CFBundleShortVersionString"),
        build: archive::text(&info, "CFBundleVersion"),
    };
    if host.identifier.is_empty() || host.version.is_empty() || host.build.is_empty() {
        return Err(
            "the app lacks CFBundleIdentifier, CFBundleShortVersionString or CFBundleVersion"
                .into(),
        );
    }
    Ok(host)
}

pub fn stamp_extension_info(info: &mut plist::Dictionary, host: &HostApp) {
    info.insert(
        "CFBundleShortVersionString".into(),
        host.version.clone().into(),
    );
    info.insert("CFBundleVersion".into(), host.build.clone().into());
}

fn stamp_extension(extension: &Path, host: &HostApp) -> Result<(), String> {
    let path = extension.join("Info.plist");
    let mut info = archive::read_plist_file(&path)?;
    stamp_extension_info(&mut info, host);
    std::fs::write(&path, archive::write_plist(info, true)?)
        .map_err(|error| format!("{}: {error}", path.display()))
}

fn sign_ad_hoc(extension: &Path) -> Result<(), String> {
    repo::run(
        extension.parent().unwrap_or(extension),
        "codesign",
        &strings(&[
            "--force",
            "--sign",
            "-",
            "--timestamp=none",
            &repo::text(extension),
        ]),
    )
    .map_err(|error| format!("signing OpenInAstral.appex failed: {error}"))
}

pub fn bundle_identifier(ipa_path: &Path) -> Result<String, String> {
    let mut ipa = Ipa::open(ipa_path)?;
    let app = ipa.app()?;
    let identifier = archive::text(&ipa.info(&app)?, "CFBundleIdentifier");
    if identifier.is_empty() {
        return Err("the app has no CFBundleIdentifier".into());
    }
    Ok(identifier)
}

pub fn add_url_scheme(ipa_path: &Path, scheme: &str) -> Result<(), String> {
    let mut ipa = Ipa::open(ipa_path)?;
    let app = ipa.app()?;
    let info_path = format!("{app}Info.plist");
    let data = ipa.read(&info_path)?;
    let mut info = archive::read_plist(&data)?;
    if !archive::ensure_url_scheme(&mut info, scheme) {
        return Ok(());
    }
    let encoded = archive::write_plist(info, archive::is_binary_plist(&data))?;
    drop(ipa);
    archive::rewrite(
        ipa_path,
        "url-scheme",
        |name| {
            if name != info_path {
                return Action::Keep;
            }
            let encoded = encoded.clone();
            Action::Patch(Box::new(move |_| Ok(encoded)))
        },
        Vec::new(),
    )
}

pub const ALTERNATE_ICONS: [&str; 2] = ["AstralIcon", "AstralMidnightIcon"];
const PHONE_SIZES: [&str; 2] = ["60x60@2x", "60x60@3x"];
const PAD_SIZES: [&str; 2] = ["76x76@2x~ipad", "83.5x83.5@2x~ipad"];

fn icon_entry(files: &[String]) -> plist::Value {
    let mut entry = plist::Dictionary::new();
    entry.insert(
        "CFBundleIconFiles".into(),
        plist::Value::Array(files.iter().cloned().map(plist::Value::String).collect()),
    );
    entry.insert("UIPrerenderedIcon".into(), plist::Value::Boolean(false));
    plist::Value::Dictionary(entry)
}

fn dictionary_at<'a>(
    parent: &'a mut plist::Dictionary,
    key: &str,
) -> Option<&'a mut plist::Dictionary> {
    if !parent
        .get(key)
        .is_some_and(|value| value.as_dictionary().is_some())
    {
        parent.insert(
            key.into(),
            plist::Value::Dictionary(plist::Dictionary::new()),
        );
    }
    parent
        .get_mut(key)
        .and_then(plist::Value::as_dictionary_mut)
}

pub fn add_icon_entries(info: &mut plist::Dictionary) {
    for (key, stems) in [
        ("CFBundleIcons", vec!["60x60"]),
        ("CFBundleIcons~ipad", vec!["60x60", "76x76", "83.5x83.5"]),
    ] {
        let Some(alternates) = dictionary_at(info, key)
            .and_then(|icons| dictionary_at(icons, "CFBundleAlternateIcons"))
        else {
            continue;
        };
        for name in ALTERNATE_ICONS {
            let files: Vec<String> = stems.iter().map(|stem| format!("{name}{stem}")).collect();
            alternates.insert(name.into(), icon_entry(&files));
        }
    }
}

fn add_alternate_icons(ipa_path: &Path) -> Result<(), String> {
    let mut ipa = Ipa::open(ipa_path)?;
    let app = ipa.app()?;
    let info_path = format!("{app}Info.plist");
    let data = ipa.read(&info_path)?;
    let mut info = archive::read_plist(&data)?;
    add_icon_entries(&mut info);
    let encoded = archive::write_plist(info, archive::is_binary_plist(&data))?;
    drop(ipa);
    let source = repo::root()?.join("packages/ios/app-icons");
    let mut additions = Vec::new();
    for name in ALTERNATE_ICONS {
        for size in PHONE_SIZES.iter().chain(PAD_SIZES.iter()) {
            let file = format!("{name}{size}.png");
            let data =
                std::fs::read(source.join(&file)).map_err(|error| format!("{file}: {error}"))?;
            additions.push(archive::Addition {
                name: format!("{app}{file}"),
                mode: 0o644,
                data,
            });
        }
    }
    archive::rewrite(
        ipa_path,
        "alternate-icons",
        |name| {
            if name != info_path {
                return Action::Keep;
            }
            let encoded = encoded.clone();
            Action::Patch(Box::new(move |_| Ok(encoded)))
        },
        additions,
    )
}

pub fn build_open_in_astral(work: &Path, host: &HostApp) -> Result<PathBuf, String> {
    if !cfg!(target_os = "macos") {
        return Err("Safari extensions require macOS".into());
    }
    let project = repo::path(&[
        "packages",
        "ios",
        "extensions",
        "OpenInAstral",
        "OpenInAstral.xcodeproj",
    ]);
    let output = work.join("OpenInAstral-build");
    repo::run(
        work,
        "xcodebuild",
        &strings(&[
            "-project",
            &repo::text(&project),
            "-quiet",
            "-target",
            "OpenInAstral",
            "build",
            "-configuration",
            "Release",
            "-sdk",
            "iphoneos",
            &format!("CONFIGURATION_BUILD_DIR={}", repo::text(&output)),
            "PRODUCT_NAME=OpenInAstral",
            &format!("PRODUCT_BUNDLE_IDENTIFIER={}.OpenInAstral", host.identifier),
            "CODE_SIGNING_REQUIRED=NO",
            "CODE_SIGNING_ALLOWED=NO",
            "ONLY_ACTIVE_ARCH=NO",
        ]),
    )
    .map_err(|error| format!("building OpenInAstral.appex failed: {error}"))?;
    let path = output.join(groups::OPEN_IN_ASTRAL);
    if !path.exists() {
        return Err("xcodebuild did not create OpenInAstral.appex".into());
    }
    stamp_extension(&path, host)?;
    sign_ad_hoc(&path)?;
    Ok(path)
}

fn absolute(path: &str) -> Result<PathBuf, String> {
    std::path::absolute(path).map_err(|error| error.to_string())
}

fn open_phoenix_access(ipa_path: &Path) -> Result<(), String> {
    let mut ipa = Ipa::open(ipa_path)?;
    let app = ipa.app()?;
    let framework = format!("{app}Frameworks/PhoenixFatClient.framework/PhoenixFatClient");
    if !ipa.has(&framework) {
        return Err(format!(
            "{} has no PhoenixFatClient.framework; astral needs a Discord build that ships Phoenix (357 or newer, TestFlight first)",
            ipa_path.display()
        ));
    }
    archive::rewrite(
        ipa_path,
        "phoenix-access",
        |name| {
            if name == framework {
                Action::Patch(Box::new(crate::phoenix::patch_access_gate))
            } else {
                Action::Keep
            }
        },
        Vec::new(),
    )
}

pub fn build(ipa: &Path, deb: &Path, destination: &Path) -> Result<(), String> {
    if loads_astral(ipa)? {
        return Err("the IPA already loads astral.dylib; use a clean Discord IPA".into());
    }
    let work = repo::scratch()?;
    let source_entries = archive::entries(ipa)?;
    let cyan = cyan_binary()?;
    let bundle = work.0.join("astral.bundle");
    stage::stage_resources(&bundle)?;
    let built = work.0.join("astral.ipa");
    let prepared = work.0.join("discord.ipa");
    files::copy(ipa, &prepared)?;
    open_phoenix_access(&prepared)?;
    repo::run(
        &work.0,
        &repo::text(&cyan),
        &strings(&[
            "-duwsgq",
            "-i",
            &repo::text(&prepared),
            "-o",
            &repo::text(&built),
            "-f",
            &repo::text(deb),
            &repo::text(&bundle),
        ]),
    )?;
    add_url_scheme(&built, URL_SCHEME)
        .map_err(|error| format!("registering astral's URL scheme failed: {error}"))?;
    add_alternate_icons(&built)
        .map_err(|error| format!("adding astral's app icons failed: {error}"))?;
    let extension = build_open_in_astral(&work.0, &host_app(ipa)?)?;
    groups::add_open_in_astral(&built, &extension)?;
    groups::add_extension_manifest(&built, &source_entries, &REQUIRED_EXTENSIONS)
        .map_err(|error| format!("recording required app extensions failed: {error}"))?;
    let remap = groups::build_group_remap(&work.0)?;
    groups::add_group_remap(&built, &remap)?;
    groups::validate_ipa_group_wiring(&built)?;
    groups::validate_ipa_extensions(
        &source_entries,
        &archive::entries(&built)?,
        &REQUIRED_EXTENSIONS,
    )?;
    files::copy(&built, destination)?;
    println!("created {}", destination.display());
    Ok(())
}

pub fn run(arguments: &[String]) -> Result<(), String> {
    let flags = Flags::new("build-ipa")
        .string("ipa", "")
        .string("deb", "")
        .string("output", &repo::text(&repo::path(&["astral.ipa"])))
        .parse(arguments)?;
    let ipa = flags.get("ipa");
    if ipa.is_empty() {
        return Err(USAGE.into());
    }
    if !Path::new(&ipa).exists() {
        return Err(format!("IPA not found: {ipa}"));
    }
    let deb = match flags.get("deb") {
        deb if deb.is_empty() => device::newest_deb()?,
        deb => absolute(&deb)?,
    };
    build(&absolute(&ipa)?, &deb, &absolute(&flags.get("output"))?)
}
