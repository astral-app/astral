use std::path::PathBuf;
use std::time::Duration;

use astral_core::settings::store::Store;
use serde_json::{Value, json};

fn directory(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("astral-settings-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).unwrap();
    directory
}

fn read(path: &PathBuf) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

#[test]
fn nested_keys_set_remove_and_prune() {
    let root = directory("nested");
    let file = root.join("settings.json");
    let mut store = Store::new(file.clone(), Duration::ZERO);
    store.load();
    assert!(store.set("plugins", "astral.bubbles.enabled", json!(true)));
    assert!(!store.set("plugins", "astral.bubbles.enabled", json!(true)));
    assert!(store.get_boolean("plugins", "astral.bubbles.enabled", false));
    assert!(store.save());
    store.remove("plugins", "astral.bubbles.enabled");
    assert!(store.save());
    assert_eq!(read(&file), json!({}));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn untouched_stores_survive_a_save() {
    let root = directory("lazy");
    let file = root.join("settings.json");
    std::fs::write(&file, br#"{"a":{"x":[1,2,{"y":"z"}]},"b":{"flag":"yes"}}"#).unwrap();
    let mut store = Store::new(file.clone(), Duration::ZERO);
    store.load();
    assert!(store.get_boolean("b", "flag", false));
    assert_eq!(store.get_string("b", "missing"), None);
    store.set("b", "other", json!(2));
    assert!(store.save());
    assert_eq!(read(&file), json!({"a":{"x":[1,2,{"y":"z"}]},"b":{"flag":"yes","other":2}}));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn corrupt_file_restores_last_good_copy() {
    let root = directory("corrupt");
    let file = root.join("settings.json");
    std::fs::write(&file, br#"{"astral":{"theme":"dark"}}"#).unwrap();
    let mut store = Store::new(file.clone(), Duration::ZERO);
    store.load();
    std::fs::write(&file, b"{broken").unwrap();
    let mut reopened = Store::new(file.clone(), Duration::ZERO);
    reopened.load();
    assert_eq!(reopened.get_string("astral", "theme").as_deref(), Some("dark"));
    assert_eq!(read(&file), json!({"astral":{"theme":"dark"}}));
    let backups = std::fs::read_dir(&root)
        .unwrap()
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().contains(".corrupt-"))
        .count();
    assert_eq!(backups, 1);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn pending_changes_merge_with_external_edits() {
    let root = directory("merge");
    let file = root.join("settings.json");
    std::fs::write(&file, br#"{"a":{"one":1}}"#).unwrap();
    let mut store = Store::new(file.clone(), Duration::ZERO);
    store.load();
    store.set("a", "two", json!(2));
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(&file, br#"{"a":{"one":1,"three":3},"b":{"kept":true}}"#).unwrap();
    assert!(store.save());
    assert_eq!(read(&file), json!({"a":{"one":1,"three":3,"two":2},"b":{"kept":true}}));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn staged_changes_clear_and_remove() {
    let root = directory("stage");
    let file = root.join("settings.json");
    std::fs::write(&file, br#"{"a":{"one":1,"two":2},"b":{"x":1}}"#).unwrap();
    let mut store = Store::new(file.clone(), Duration::ZERO);
    store.load();
    let changes = json!([{"store":"b","clear":true},{"store":"a","key":"one","remove":true},{"store":"c","key":"d.e","value":"f"}]);
    assert!(store.stage(changes.to_string().as_bytes()));
    assert!(store.save());
    assert_eq!(read(&file), json!({"a":{"two":2},"c":{"d":{"e":"f"}}}));
    std::fs::remove_dir_all(root).unwrap();
}
