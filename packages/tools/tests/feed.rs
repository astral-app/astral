use std::collections::{BTreeMap, BTreeSet};

use astral_tools::feed::{IpaFacts, add_version};
use serde_json::json;

fn facts(build: &str, entitlement: &str) -> IpaFacts {
    IpaFacts {
        version: "357.0".into(),
        build: build.into(),
        minimum_os: "18.0".into(),
        size: 10,
        entitlements: BTreeSet::from([entitlement.to_owned()]),
        privacy: BTreeMap::from([("NSCameraUsageDescription".to_owned(), "camera".to_owned())]),
    }
}

#[test]
fn adds_newest_first_and_unions_permissions() {
    let mut source = json!({ "apps": [{ "versions": [], "appPermissions": { "entitlements": ["old"], "privacy": {} } }] });
    add_version(
        &mut source,
        &facts("1", "a"),
        "https://x/1.ipa",
        "2026-11-26",
        "one",
    )
    .unwrap();
    add_version(
        &mut source,
        &facts("2", "b"),
        "https://x/2.ipa",
        "2026-11-27",
        "two",
    )
    .unwrap();
    add_version(
        &mut source,
        &facts("2", "b"),
        "https://x/2.ipa",
        "2026-11-27",
        "two again",
    )
    .unwrap();
    let app = &source["apps"][0];
    assert_eq!(app["versions"].as_array().unwrap().len(), 2);
    assert_eq!(app["versions"][0]["buildVersion"], "2");
    assert_eq!(app["versions"][0]["localizedDescription"], "two again");
    assert_eq!(
        app["appPermissions"]["entitlements"],
        json!(["a", "b", "old"])
    );
    assert_eq!(
        app["appPermissions"]["privacy"]["NSCameraUsageDescription"],
        "camera"
    );
}

#[test]
fn drops_builds_without_phoenix() {
    let mut source = json!({ "apps": [{ "versions": [{ "version": "355.0", "downloadURL": "https://x/old.ipa" }] }] });
    let mut old = facts("1", "a");
    old.version = "356.7".into();
    assert!(add_version(&mut source, &old, "https://x/356.ipa", "2026-11-27", "old").is_err());
    add_version(
        &mut source,
        &facts("2", "b"),
        "https://x/2.ipa",
        "2026-11-27",
        "two",
    )
    .unwrap();
    let versions = source["apps"][0]["versions"].as_array().unwrap();
    assert_eq!(versions.len(), 1);
    assert_eq!(versions[0]["version"], "357.0");
}
