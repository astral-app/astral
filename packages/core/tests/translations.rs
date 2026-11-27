use astral_core::support::translations::{choose, locales, tr, tr_with};

fn keys_of(locale: &str) -> Vec<String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("i18n/{locale}.toml"));
    let table: toml::Table = std::fs::read_to_string(path).expect("locale file").parse().expect("locale toml");
    table.keys().cloned().collect()
}

#[test]
fn every_locale_uses_only_english_keys() {
    let english = keys_of("en-US");
    for locale in locales() {
        for key in keys_of(&locale) {
            assert!(english.contains(&key), "{locale} has {key}, which en-US lacks");
        }
    }
}

#[test]
fn picks_the_preferred_locale_and_falls_back_to_english() {
    assert_eq!(choose(&["de-AT".to_owned()]), Some("de"));
    assert_eq!(tr("ASTRAL_SETTINGS"), "Einstellungen");
    assert_eq!(tr("ASTRAL_NOT_A_KEY"), "ASTRAL_NOT_A_KEY");
    assert_eq!(choose(&["xx".to_owned()]), Some("en-US"));
    assert_eq!(tr_with("ASTRAL_FILTER_ALL", &[("count", "3".to_owned())]), "All 3");
}
