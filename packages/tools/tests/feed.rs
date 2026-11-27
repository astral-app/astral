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
