use astral_core::reactors::{encoded, rate_limit_wait, users};
use std::time::Duration;

#[test]
fn encodes_emoji_for_the_path() {
    assert_eq!(encoded("check:123"), "check%3A123");
    assert_eq!(encoded("👍"), "%F0%9F%91%8D");
}

#[test]
fn keeps_up_to_five_users_with_their_public_fields() {
    let body =
        br#"[{"id":"1","username":"a","avatar":"x","email":"no"},{"name":"skip"},{"id":"2"},{"id":"3"},{"id":"4"},{"id":"5"},{"id":"6"}]"#;
    let list = users(body).expect("users");
    let list = list.as_array().expect("array");
    assert_eq!(list.len(), 5);
    assert_eq!(list[0], serde_json::json!({"id": "1", "username": "a", "avatar": "x"}));
    assert!(users(b"[]").is_none());
    assert!(users(b"{}").is_none());
}

#[test]
fn caps_the_rate_limit_wait() {
    assert_eq!(rate_limit_wait(br#"{"retry_after":1.5}"#), Duration::from_millis(1500));
    assert_eq!(rate_limit_wait(br#"{"retry_after":60}"#), Duration::from_secs(10));
    assert_eq!(rate_limit_wait(b"nope"), Duration::ZERO);
}
