use astral_core::plugins::storage::{OVER_LIMIT, REMOVED, SAVED, Storage, UNAVAILABLE};
use serde_json::json;

fn scratch(name: &str) -> String {
    let directory = std::env::temp_dir().join(format!("astral-storage-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).unwrap();
    directory.join("storage.db").to_string_lossy().into_owned()
}

#[test]
fn saves_reads_and_lists_keys_in_order() {
    let storage = Storage::open(&scratch("order"), 512 << 10);
    assert!(storage.available());
    assert_eq!(storage.set("shiki", "theme", &json!({"name": "dark"})), SAVED);
    assert_eq!(storage.set("shiki", "alpha", &json!(1)), SAVED);
    assert_eq!(storage.set("shiki", "theme", &json!({"name": "light"})), SAVED);
    assert_eq!(storage.get("shiki", "theme"), Some(json!({"name": "light"})));
    assert_eq!(storage.keys("shiki"), vec!["alpha", "theme"]);
    assert_eq!(storage.get("other", "theme"), None);
}

#[test]
fn null_removes_and_remove_all_clears_one_plugin() {
    let storage = Storage::open(&scratch("remove"), 512 << 10);
    storage.set("a", "one", &json!(true));
    storage.set("b", "one", &json!(true));
    assert_eq!(storage.set("a", "one", &serde_json::Value::Null), REMOVED);
    assert_eq!(storage.get("a", "one"), None);
    storage.set("a", "two", &json!(2));
    assert!(storage.remove_all("a"));
    assert!(storage.keys("a").is_empty());
    assert_eq!(storage.keys("b"), vec!["one"]);
}

#[test]
fn enforces_the_per_plugin_limit_excluding_the_replaced_key() {
    let storage = Storage::open(&scratch("limit"), 32);
    assert_eq!(storage.set("p", "k", &json!("0123456789012345678901")), SAVED);
    assert_eq!(storage.set("p", "k", &json!("abcdefghijabcdefghijab")), SAVED);
    assert_eq!(storage.set("p", "x", &json!("0123456789")), OVER_LIMIT);
    assert_eq!(storage.set("q", "x", &json!("0123456789")), SAVED);
}

#[test]
fn reports_unavailable_when_the_path_cannot_open() {
    let storage = Storage::open("/nonexistent-astral/dir/storage.db", 1024);
    assert!(!storage.available());
    assert_eq!(storage.set("p", "k", &json!(1)), UNAVAILABLE);
    assert_eq!(storage.get("p", "k"), None);
}
