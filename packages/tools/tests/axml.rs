use astral_tools::axml::add_scheme_filter;

#[test]
fn patches_a_manifest_from_the_environment() {
    let Ok(path) = std::env::var("ASTRAL_TEST_MANIFEST") else {
        return;
    };
    let manifest = std::fs::read(&path).expect("manifest");
    let patched = add_scheme_filter(&manifest, "com.discord.main.MainActivity", "astral")
        .expect("patch")
        .expect("changed");
    assert!(
        add_scheme_filter(&patched, "com.discord.main.MainActivity", "astral")
            .expect("repatch")
            .is_none()
    );
    std::fs::write(format!("{path}.patched"), patched).expect("write");
}

#[test]
fn refuses_text_that_is_not_a_binary_manifest() {
    assert!(add_scheme_filter(b"<manifest/>", "a", "astral").is_err());
}
