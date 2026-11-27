#[test]
fn only_lspatch_copies_are_redundant_in_splits() {
    use astral_tools::android::redundant_in_split;
    assert!(redundant_in_split("assets/lspatch/origin.apk"));
    assert!(redundant_in_split(
        "assets/lspatch/modules/app.astral.android.apk"
    ));
    assert!(!redundant_in_split("assets/lspatch/config.json"));
    assert!(!redundant_in_split("assets/index.android.bundle"));
    assert!(!redundant_in_split("lib/arm64-v8a/libdiscord.so"));
}

#[test]
fn recognises_config_splits_by_name() {
    use astral_tools::android::is_config_split;
    assert!(is_config_split("config.arm64_v8a-487-lspatched.apk"));
    assert!(!is_config_split("base-487-lspatched.apk"));
}
