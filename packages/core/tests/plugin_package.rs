use std::fs;
use std::path::PathBuf;

use astral_core::plugin_package::{Limits, best_locale, fill_placeholders, load, valid_plugin_identifier};

fn plugin_directory(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("astral-package-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("i18n")).unwrap();
    fs::create_dir_all(root.join("lib/deep")).unwrap();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(
        root.join("manifest.toml"),
        r#"id = "astral.sample"
name = "Sample Plugin"
version = "1.0.0"
permissions = ["storage", "network", "storage"]
enabled_by_default = true
authors = ["solo", { name = "adrian", id = "224617799434108928" }]
"#,
    )
    .unwrap();
    fs::write(root.join("main.lua"), "return 1").unwrap();
    fs::write(root.join("main.luac"), [0u8, 1, 2]).unwrap();
    fs::write(root.join("lib/util.lua"), "return 2").unwrap();
    fs::write(root.join("lib/deep/inner.lua"), "return 3").unwrap();
    fs::write(root.join("tests/skip-test.lua"), "error()").unwrap();
    fs::write(root.join("i18n/en-US.toml"), "HELLO = \"Hello {name}\"\nONLY = \"english\"\n").unwrap();
    fs::write(root.join("i18n/de.toml"), "HELLO = \"Hallo {name}\"\n").unwrap();
    root
}

#[test]
fn loads_a_plugin_package() {
    let root = plugin_directory("load");
    let package = load(root.to_str().unwrap(), &["de-AT".to_owned()], Limits::default()).expect("package");
    assert_eq!(package.identifier, "astral.sample");
    assert_eq!(package.permissions, vec!["network", "storage"]);
    assert!(package.enabled_by_default);
    assert_eq!(
        package.authors,
        vec![("solo".into(), "".into()), ("adrian".into(), "224617799434108928".into())]
    );
    assert_eq!(package.main_bytecode, vec![0, 1, 2]);
    assert_eq!(package.locale, "de");
    assert_eq!(package.strings.get("HELLO").map(String::as_str), Some("Hallo {name}"));
    assert_eq!(package.fallback_strings.get("ONLY").map(String::as_str), Some("english"));
    let names: Vec<&String> = package.modules.keys().collect();
    assert_eq!(names, vec!["lib.deep.inner", "lib.util"]);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn rejects_bad_packages() {
    let root = plugin_directory("reject");
    let tiny = Limits {
        modules: 10,
        ..Limits::default()
    };
    assert!(load(root.to_str().unwrap(), &[], tiny).is_none());
    fs::write(root.join("manifest.toml"), "id = \"Bad ID\"\nname = \"x\"\n").unwrap();
    assert!(load(root.to_str().unwrap(), &[], Limits::default()).is_none());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn locale_helpers() {
    let list = |items: &[&str]| items.iter().map(|item| item.to_string()).collect::<Vec<_>>();
    assert_eq!(best_locale(&list(&["en-US", "de"]), &list(&["de-AT"])).as_deref(), Some("de"));
    assert_eq!(best_locale(&list(&["en-GB", "fr"]), &list(&["ja"])).as_deref(), Some("en-GB"));
    assert_eq!(best_locale(&list(&["fr"]), &list(&["ja"])), None);
    assert_eq!(
        fill_placeholders("{a} and {a}", &[("a".into(), "{a}{a}".into())]),
        "{a}{a} and {a}{a}"
    );
    assert!(valid_plugin_identifier("astral.view-raw"));
    assert!(!valid_plugin_identifier("Astral"));
    assert!(!valid_plugin_identifier("a..b"));
}
