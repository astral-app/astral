use std::time::Duration;

use astral_core::plugins::backup::{export, import};
use astral_core::settings::store::Store;
use serde_json::json;

#[test]
fn backups_round_trip_settings_and_plugin_values() {
    let root = std::env::temp_dir().join(format!("astral-backup-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let mut source = Store::new(root.join("source.json"), Duration::ZERO);
    source.load();
    source.set("astral", "theme", json!("mana"));
    source.set("astral", "enabledPlugin", json!("skip"));
    source.set("astral", "scale", json!(1.25));
    source.set("plugins", "astral.bubbles.setting.color", json!("#112233"));
    source.set("plugins", "astral.bubbles.setting.radius", json!(16));
    let plugins = json!([
        {"id": "astral.bubbles", "enabled": true, "bundled": true, "switches": [{"key": "color"}, {"key": "radius"}, {"key": "unset"}]},
        {"id": "astral.local", "enabled": false, "switches": []}
    ]);
    let themes = json!([{"id": "discord", "bundled": true}, {"id": "mana"}]);
    let text = export(&mut source, "0.0.1", &plugins, &themes);
    assert!(!text.contains("enabledPlugin"));
    let mut target = Store::new(root.join("target.json"), Duration::ZERO);
    target.load();
    let installs = import(&mut target, text.as_bytes(), &json!([]), &json!([])).unwrap();
    assert_eq!(installs, json!([]));
    assert_eq!(target.get_string("astral", "theme").as_deref(), Some("mana"));
    assert_eq!(target.get("astral", "scale"), Some(json!(1.25)));
    assert_eq!(target.get("astral", "enabledPlugin"), None);
    assert_eq!(target.get("plugins", "astral.bubbles.setting.color"), Some(json!("#112233")));
    assert_eq!(target.get("plugins", "astral.bubbles.setting.radius"), Some(json!(16)));
    assert!(target.get_boolean("plugins", "astral.bubbles.enabled", false));
    assert!(!target.get_boolean("plugins", "astral.local.enabled", true));
    assert_eq!(
        import(&mut target, b"format = 2", &json!([]), &json!([])),
        Err("not an astral backup")
    );
    std::fs::remove_dir_all(root).unwrap();
}
