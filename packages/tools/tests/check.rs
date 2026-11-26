use astral_tools::check;

fn planned(path: &str, exists: bool) -> String {
    let rules = check::rules();
    check::plan(&rules, &[path.to_owned()], |_| exists)
        .iter()
        .map(|(rule, _)| rule.name)
        .collect::<Vec<_>>()
        .join(",")
}

#[test]
fn picks_affected_steps() {
    for (path, want) in [
        (
            "packages/core/wire/PhoenixWire.h",
            "clang-format,wire tests",
        ),
        (
            "packages/core/lua/host/mod.rs",
            "rust,rust magic numbers,lua tests",
        ),
        ("packages/tools/cli/check.rs", "rust,rust magic numbers"),
        (
            "packages/ios/sources/Themes.mm",
            "clang-format,objc boolean boxing",
        ),
        ("README.md", ""),
    ] {
        assert_eq!(planned(path, true), want, "{path}");
    }
}

#[test]
fn skips_file_scoped_steps_for_deletions() {
    assert_eq!(planned("packages/core/wire/Removed.h", false), "wire tests");
}
