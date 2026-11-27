use astral_tools::release_notes::{notes, parse};

fn subjects(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|line| (*line).to_owned()).collect()
}

#[test]
fn parses_conventional_subjects() {
    let change = parse("fix(core): stop typing pulses (#208)").unwrap();
    assert_eq!(
        (change.kind, change.scope, change.summary),
        ("fix", Some("core"), "stop typing pulses (#208)")
    );
    assert!(parse("Merge branch main").is_none());
    assert_eq!(parse("feat!: breaking").unwrap().kind, "feat");
}

#[test]
fn groups_changes_and_counts_internal_ones() {
    let text = notes(&subjects(&[
        "feat(ios): a",
        "fix: b",
        "perf(core): c",
        "ci: d",
        "refactor(core): e",
        "not conventional",
    ]));
    assert!(text.contains("### Added\n\n- ios: a"));
    assert!(text.contains("### Fixed\n\n- b"));
    assert!(text.contains("### Faster\n\n- core: c"));
    assert!(text.ends_with("2 internal changes (refactors, builds, CI, docs)."));
    assert_eq!(
        notes(&subjects(&["ci: x"])),
        "1 internal changes (refactors, builds, CI, docs)."
    );
    assert_eq!(notes(&[]), "No user-facing changes since the last alpha.");
}
