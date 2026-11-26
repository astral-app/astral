use astral_core::capability::Capability;

#[test]
fn every_capability_round_trips_and_is_described() {
    for capability in Capability::ALL {
        assert_eq!(Capability::parse(capability.value()), Some(*capability));
        assert!(!capability.description().is_empty(), "{capability:?} has no description");
        assert!(capability.icon().starts_with("res://"), "{capability:?} has no icon");
    }
}

#[test]
fn every_plugin_permission_is_a_capability() {
    let plugins = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../addons/plugins");
    for entry in std::fs::read_dir(plugins).expect("plugins").flatten() {
        let Ok(manifest) = std::fs::read_to_string(entry.path().join("manifest.toml")) else {
            continue;
        };
        let parsed: toml::Table = manifest.parse().expect("manifest");
        let permissions = parsed
            .get("permissions")
            .and_then(toml::Value::as_array)
            .cloned()
            .unwrap_or_default();
        for permission in permissions.iter().filter_map(toml::Value::as_str) {
            assert!(
                Capability::parse(permission).is_some(),
                "{}: unknown permission {permission}",
                entry.path().display()
            );
        }
    }
}
