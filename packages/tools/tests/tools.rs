use std::collections::BTreeMap;
use std::time::{Duration, SystemTime};

use astral_tools::compat::{CompatResult, Probe};
use astral_tools::sideload::Profile;
use astral_tools::{compat, create, doctor, macho, palette, repo};
use plist::{Dictionary, Value};

#[test]
fn loaded_dylibs_read_load_commands() {
    let executable = std::fs::read(std::env::current_exe().expect("executable")).expect("binary");
    let Ok(file) = macho::parse(&executable) else {
        return;
    };
    let libraries = file.loaded_dylibs();
    assert!(
        libraries
            .iter()
            .any(|library| library.starts_with("/usr/lib/libSystem")),
        "{libraries:?}"
    );
}

fn result(version: &str, uuid: &str, text: u16) -> CompatResult {
    CompatResult {
        phoenix: true,
        version: version.into(),
        build: "1".into(),
        phoenix_uuid: uuid.into(),
        supported: true,
        probe: Probe {
            properties: BTreeMap::from([("text".to_owned(), text), ("uri".to_owned(), 542)]),
            ..Probe::default()
        },
        ..CompatResult::default()
    }
}

#[test]
fn compat_report_lists_moved_properties() {
    let report = compat::report(&result("357.0", "b", 501), Some(&result("356.0", "a", 499)));
    assert!(report.contains("`text` 499 → 501"), "{report}");
    assert!(!report.contains("`uri`"), "{report}");
}

#[test]
fn compat_seam_diff_lists_added_removed_and_gate_changes() {
    let mut before = result("356.0", "a", 499);
    before.access_gate = 2;
    before.probe.host_call = 1;
    let mut after = result("357.0", "b", 499);
    after.probe.properties.remove("uri");
    after.probe.properties.insert("src".to_owned(), 471);
    after.access_gate = 0;
    let report = compat::report(&after, Some(&before));
    assert!(report.contains("View properties added: `src`"), "{report}");
    assert!(
        report.contains("View properties removed: `uri`"),
        "{report}"
    );
    assert!(report.contains("Host-call entry is gone"), "{report}");
    assert!(report.contains("Access gate evaluators 2 → 0"), "{report}");
    assert!(report.contains("**not found**"), "{report}");
    assert!(compat::seam_diff(&before, &before).is_empty());
}

#[test]
fn scaffold_manifest_is_valid_toml() {
    let manifest: toml::Table = create::manifest("astral.sample", "Sample", "adrian")
        .parse()
        .expect("toml");
    assert_eq!(manifest["id"].as_str(), Some("astral.sample"));
    assert_eq!(manifest["main"].as_str(), Some("main.lua"));
    assert!(manifest.contains_key("permissions"));
}

fn profile(expiration: SystemTime, groups: &[&str]) -> Profile {
    let mut entitlements = Dictionary::new();
    entitlements.insert(
        "application-identifier".into(),
        Value::String("TEAM.dev.example.*".into()),
    );
    entitlements.insert(
        "com.apple.security.application-groups".into(),
        Value::Array(
            groups
                .iter()
                .map(|group| Value::String((*group).into()))
                .collect(),
        ),
    );
    Profile {
        expiration: Some(expiration),
        devices: vec!["00008030-ABC".into()],
        team: vec!["TEAM".into()],
        entitlements,
        ..Profile::default()
    }
}

#[test]
fn profile_covers_bundle_and_device() {
    let current = profile(SystemTime::now() + Duration::from_secs(3600), &[]);
    assert!(current.covers("dev.example.app", "00008030-abc"));
    assert!(!current.covers("com.other.app", "00008030-ABC"));
    assert!(!current.covers("dev.example.app", "OTHER"));
    let expired = profile(SystemTime::now() - Duration::from_secs(3600), &[]);
    assert!(!expired.covers("dev.example.app", "00008030-ABC"));
}

#[test]
fn profile_finds_the_signed_application_group() {
    let current = profile(
        SystemTime::now() + Duration::from_secs(3600),
        &["", "group.dev.example.discord", "group.dev.example.other"],
    );
    assert_eq!(
        current.application_groups(),
        vec![
            "group.dev.example.discord".to_owned(),
            "group.dev.example.other".to_owned()
        ]
    );
    assert!(current.covers("dev.example.discord.Share", "00008030-abc"));
}

#[test]
fn parses_semantic_colors() {
    let source = "r1 = {'SemanticColors': null};
        r5 = {'TEXT_DEFAULT': null, 'BACKGROUND_BASE_LOW': null};
        r6 = {'category': 'generic'};
        r9 = r7.DARK;
        r8 = {'raw': 'NEUTRAL_5', 'opacity': 1};
        r6[9] = r8;
        r9 = r7.LIGHT;
        r8 = {'raw': 'NEUTRAL_72', 'opacity': 0.5};
        r6[9] = r8;
        r5[0] = r6;
        r6 = {'category': 'generic'};
        r9 = r7.DARK;
        r8 = {'raw': 'NEUTRAL_82', 'opacity': 1};
        r6[9] = r8;
        r5[1] = r6;";
    let (keys, entries) = palette::parse(source).expect("parsed");
    let pair = |raw: &str, opacity: &str| (raw.to_owned(), opacity.to_owned());
    assert_eq!(keys.len(), 2);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[&0]["dark"], pair("NEUTRAL_5", "1"));
    assert_eq!(entries[&0]["light"], pair("NEUTRAL_72", "0.5"));
    assert_eq!(entries[&1]["dark"], pair("NEUTRAL_82", "1"));
    let rendered = palette::render(&keys, &entries, "test");
    assert!(
        rendered.contains(r#"BACKGROUND_BASE_LOW = { dark = ["NEUTRAL_82", 1] }"#),
        "{rendered}"
    );
}

#[test]
fn doctor_reads_ci_pins() {
    let workflow = repo::read(&repo::path(&[".github", "workflows", "ci.yml"])).expect("workflow");
    let pins = doctor::pins_from(&workflow);
    for (binary, _) in doctor::PINNED_TOOLS {
        assert!(
            pins.get(binary).is_some_and(|pin| !pin.is_empty()),
            "no CI pin for {binary}"
        );
    }
}

#[test]
fn doctor_matches_versions() {
    for (output, pinned, want) in [
        ("clang-format version 23.1.5", "23.1.5", true),
        ("clang-format version 23.1.6", "23.1.5", false),
        ("stylua 2.6.0", "2.6.0", true),
        ("stylua 12.6.0", "2.6.0", false),
        ("3.19.2", "3.19.2", true),
        ("3.19.20", "3.19.2", false),
    ] {
        assert_eq!(
            doctor::version_matches(output, pinned),
            want,
            "{output} {pinned}"
        );
    }
    assert!(doctor::version_matches(
        "clang-format version 23.1.6",
        &format!("{}.[0-9]+", doctor::minor_of("23.1.5"))
    ));
}
