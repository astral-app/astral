mod support;

use std::path::Path;

use astral_tools::ipa;
use astral_tools::ipa_archive::{self as archive};
use astral_tools::ipa_groups::{self as groups, GROUP_REMAP_FRAMEWORK, GROUP_REMAP_INSTALL};
use astral_tools::macho;
use astral_tools::sideload;
use plist::Value;
use support::{bundle_binary, extension_info, group_dylib, plist, read_zip, temporary, write_zip};

const APP: &str = "Payload/Discord.app/";

fn entry(path: &str) -> String {
    format!("{APP}{path}")
}

#[test]
fn adds_weak_dylib_idempotently() {
    let original = bundle_binary();
    let patched = macho::add_weak_dylib(&original, GROUP_REMAP_INSTALL).expect("patched");
    assert!(macho::loads_dylib(&patched, GROUP_REMAP_INSTALL));
    assert_eq!(
        macho::add_weak_dylib(&patched, GROUP_REMAP_INSTALL).expect("second"),
        patched
    );
    assert!(macho::add_weak_dylib(&original[..32], GROUP_REMAP_INSTALL).is_err());
}

#[test]
fn group_remap_wires_discord_extensions_only() {
    let directory = temporary("groups");
    let ipa = directory.0.join("Discord.ipa");
    let names = [
        ("Share", "com.hammerandchisel.discord.Share"),
        (
            "BroadcastUpload",
            "com.hammerandchisel.discord.BroadcastUpload",
        ),
        (
            "NotificationService",
            "com.hammerandchisel.discord.NotificationService",
        ),
        ("OpenInAstral", "com.hammerandchisel.discord.OpenInAstral"),
    ];
    let mut entries = vec![
        (
            entry("Info.plist"),
            plist(&[
                (
                    "CFBundleIdentifier",
                    Value::String("com.hammerandchisel.discord".into()),
                ),
                ("CFBundleExecutable", Value::String("Discord".into())),
            ]),
        ),
        (entry("Discord"), bundle_binary()),
        (
            entry("PlugIns/OpenInAstral.appex/manifest.json"),
            b"{\"manifest_version\":3}".to_vec(),
        ),
    ];
    for (name, identifier) in names {
        entries.push((
            entry(&format!("PlugIns/{name}.appex/Info.plist")),
            extension_info(identifier, name),
        ));
        entries.push((
            entry(&format!("PlugIns/{name}.appex/{name}")),
            bundle_binary(),
        ));
    }
    let borrowed: Vec<(&str, Vec<u8>)> = entries
        .iter()
        .map(|(name, data)| (name.as_str(), data.clone()))
        .collect();
    write_zip(&ipa, &borrowed);
    let library = directory.0.join("AstralGroups.dylib");
    std::fs::write(&library, group_dylib(GROUP_REMAP_INSTALL)).expect("library");
    groups::add_group_remap(&ipa, &library).expect("remap");
    let patched = read_zip(&ipa);
    groups::validate_group_remap_library(&patched[&entry(GROUP_REMAP_FRAMEWORK)])
        .expect("embedded library");
    for name in ["Share", "BroadcastUpload", "NotificationService"] {
        let binary = &patched[&entry(&format!("PlugIns/{name}.appex/{name}"))];
        assert!(macho::loads_dylib(binary, GROUP_REMAP_INSTALL), "{name}");
    }
    assert!(!macho::loads_dylib(
        &patched[&entry("PlugIns/OpenInAstral.appex/OpenInAstral")],
        GROUP_REMAP_INSTALL
    ));
    groups::validate_ipa_group_wiring(&ipa).expect("wiring");
}

#[test]
fn open_in_astral_replaces_the_existing_extension() {
    let directory = temporary("extension");
    let ipa = directory.0.join("Discord.ipa");
    write_zip(
        &ipa,
        &[
            (
                &entry("Info.plist"),
                plist(&[(
                    "CFBundleIdentifier",
                    Value::String("com.hammerandchisel.discord".into()),
                )]),
            ),
            (&entry("PlugIns/OpenInAstral.appex/old"), b"old".to_vec()),
        ],
    );
    let extension = directory.0.join("OpenInAstral.appex");
    std::fs::create_dir_all(&extension).expect("extension");
    std::fs::write(
        extension.join("Info.plist"),
        extension_info("com.hammerandchisel.discord.OpenInAstral", "OpenInAstral"),
    )
    .expect("info");
    std::fs::write(extension.join("OpenInAstral"), bundle_binary()).expect("binary");
    std::fs::write(extension.join("manifest.json"), b"{\"manifest_version\":3}").expect("manifest");
    groups::add_open_in_astral(&ipa, &extension).expect("added");
    let entries = read_zip(&ipa);
    assert!(!entries.contains_key(&entry("PlugIns/OpenInAstral.appex/old")));
    assert_eq!(
        entries[&entry("PlugIns/OpenInAstral.appex/manifest.json")],
        b"{\"manifest_version\":3}"
    );
}

fn scheme_counts(info: &plist::Dictionary) -> std::collections::BTreeMap<String, usize> {
    let mut counts = std::collections::BTreeMap::new();
    for kind in info["CFBundleURLTypes"].as_array().expect("types") {
        for scheme in kind
            .as_dictionary()
            .and_then(|kind| kind.get("CFBundleURLSchemes"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            *counts
                .entry(scheme.as_string().unwrap_or_default().to_owned())
                .or_default() += 1;
        }
    }
    counts
}

#[test]
fn url_scheme_is_added_once() {
    let directory = temporary("scheme");
    let ipa = directory.0.join("Discord.ipa");
    let mut existing = plist::Dictionary::new();
    existing.insert(
        "CFBundleURLName".into(),
        Value::String("com.hammerandchisel.discord".into()),
    );
    existing.insert(
        "CFBundleURLSchemes".into(),
        Value::Array(vec![
            Value::String("discord".into()),
            Value::String("com.hammerandchisel.discord".into()),
        ]),
    );
    write_zip(
        &ipa,
        &[(
            &entry("Info.plist"),
            plist(&[
                (
                    "CFBundleIdentifier",
                    Value::String("com.hammerandchisel.discord".into()),
                ),
                (
                    "CFBundleURLTypes",
                    Value::Array(vec![Value::Dictionary(existing)]),
                ),
            ]),
        )],
    );
    ipa::add_url_scheme(&ipa, ipa::URL_SCHEME).expect("first");
    ipa::add_url_scheme(&ipa, ipa::URL_SCHEME).expect("second");
    let info = archive::read_plist(&read_zip(&ipa)[&entry("Info.plist")]).expect("info");
    let counts = scheme_counts(&info);
    assert_eq!(counts.get("discord"), Some(&1));
    assert_eq!(counts.get("com.hammerandchisel.discord"), Some(&1));
    assert_eq!(counts.get(ipa::URL_SCHEME), Some(&1));
}

#[test]
fn validates_extension_sets() {
    let source = vec![
        entry("PlugIns/BroadcastUpload.appex/Info.plist"),
        entry("PlugIns/Share.appex/Info.plist"),
    ];
    let mut complete = source.clone();
    complete.push(entry("PlugIns/OpenInAstral.appex/Info.plist"));
    groups::validate_ipa_extensions(&source, &complete, &["OpenInAstral.appex"]).expect("complete");
    let dropped = groups::validate_ipa_extensions(&source, &source[..1], &[]).expect_err("dropped");
    assert!(dropped.contains("Share.appex"));
    let omitted = groups::validate_ipa_extensions(&source, &source, &["OpenInAstral.appex"])
        .expect_err("omitted");
    assert!(omitted.contains("OpenInAstral.appex"));
}

#[test]
fn manifest_records_source_and_astral_extensions() {
    let directory = temporary("manifest");
    let ipa = directory.0.join("Discord.ipa");
    let extensions = ["Share", "BroadcastUpload", "NotificationService"];
    let mut source = vec![entry("Info.plist")];
    let mut entries = vec![(
        entry("Info.plist"),
        plist(&[(
            "CFBundleIdentifier",
            Value::String("com.hammerandchisel.discord".into()),
        )]),
    )];
    for name in extensions {
        let path = entry(&format!("PlugIns/{name}.appex/Info.plist"));
        source.push(path.clone());
        entries.push((
            path,
            extension_info(&format!("com.hammerandchisel.discord.{name}"), name),
        ));
    }
    let borrowed: Vec<(&str, Vec<u8>)> = entries
        .iter()
        .map(|(name, data)| (name.as_str(), data.clone()))
        .collect();
    write_zip(&ipa, &borrowed);
    groups::add_extension_manifest(&ipa, &source, &groups::REQUIRED_EXTENSIONS).expect("manifest");
    let manifest: serde_json::Value =
        serde_json::from_slice(&read_zip(&ipa)[&entry("AstralExtensions.json")]).expect("json");
    assert_eq!(
        manifest["extensions"],
        serde_json::json!([
            "BroadcastUpload.appex",
            "NotificationService.appex",
            "OpenInAstral.appex",
            "Share.appex"
        ])
    );
}

#[test]
fn signed_app_reports_missing_manifest_extensions() {
    let directory = temporary("signed-manifest");
    let app = &directory.0;
    std::fs::write(
        app.join("AstralExtensions.json"),
        br#"{"extensions":["BroadcastUpload.appex","OpenInAstral.appex","Share.appex"]}"#,
    )
    .expect("manifest");
    for extension in ["OpenInAstral.appex", "Share.appex"] {
        std::fs::create_dir_all(app.join("PlugIns").join(extension)).expect("bundle");
    }
    assert!(
        groups::validate_signed_extensions(app)
            .expect_err("missing")
            .contains("BroadcastUpload.appex")
    );
    std::fs::create_dir_all(app.join("PlugIns/BroadcastUpload.appex")).expect("bundle");
    groups::validate_signed_extensions(app).expect("complete");
}

fn write_signed_extension(app: &Path, name: &str, identifier: &str, binary: Vec<u8>) {
    let bundle = app.join("PlugIns").join(format!("{name}.appex"));
    std::fs::create_dir_all(&bundle).expect("bundle");
    std::fs::write(bundle.join("Info.plist"), extension_info(identifier, name)).expect("info");
    std::fs::write(bundle.join(name), binary).expect("binary");
}

#[test]
fn signed_app_rejects_unwired_discord_extension() {
    let directory = temporary("signed-groups");
    let app = &directory.0;
    let app_id = "com.astral.client";
    std::fs::write(
        app.join("Info.plist"),
        plist(&[
            ("CFBundleIdentifier", Value::String(app_id.into())),
            ("CFBundleExecutable", Value::String("Discord".into())),
        ]),
    )
    .expect("info");
    std::fs::write(app.join("Discord"), bundle_binary()).expect("binary");
    std::fs::create_dir_all(app.join("Frameworks")).expect("frameworks");
    std::fs::write(
        app.join(GROUP_REMAP_FRAMEWORK),
        group_dylib(GROUP_REMAP_INSTALL),
    )
    .expect("library");
    let wired = || macho::add_weak_dylib(&bundle_binary(), GROUP_REMAP_INSTALL).expect("wired");
    write_signed_extension(app, "Share", &format!("{app_id}.Share"), bundle_binary());
    write_signed_extension(
        app,
        "BroadcastUpload",
        &format!("{app_id}.BroadcastUpload"),
        wired(),
    );
    write_signed_extension(
        app,
        "OpenInAstral",
        &format!("{app_id}.OpenInAstral"),
        bundle_binary(),
    );
    assert!(
        groups::validate_signed_group_wiring(app)
            .expect_err("unwired")
            .contains("Share.appex")
    );
    write_signed_extension(app, "Share", &format!("{app_id}.Share"), wired());
    groups::validate_signed_group_wiring(app).expect("wired");
}

#[test]
fn extension_bundle_id_uses_the_parent_identifier() {
    assert_eq!(
        sideload::extension_bundle_id("com.discord", "app.astral.discord", "com.discord.Share")
            .expect("mapped"),
        "app.astral.discord.Share"
    );
    assert!(
        sideload::extension_bundle_id("com.discord", "app.astral.discord", "com.other.Share")
            .is_err()
    );
}
