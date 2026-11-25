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

#[test]
fn keeps_discord_request_headers() {
    use astral_core::discord_headers::{api_version, clear, headers, note};
    clear();
    note(
        "discord.com",
        "/api/v10/users/@me",
        &[("Authorization".into(), "x".into()), ("User-Agent".into(), "Discord".into())],
    );
    assert!(headers().is_empty());
    note(
        "Discord.com",
        "/api/v10/users/@me",
        &[
            ("Authorization".into(), "x".into()),
            ("X-Super-Properties".into(), "abc".into()),
            ("User-Agent".into(), "Discord".into()),
            ("X-Context-Properties".into(), "skip".into()),
        ],
    );
    assert_eq!(api_version(), 10);
    assert_eq!(
        headers(),
        vec![
            ("x-super-properties".to_owned(), "abc".to_owned()),
            ("user-agent".to_owned(), "Discord".to_owned())
        ]
    );
    note("cdn.discordapp.com", "/api/v9/x", &[("X-Super-Properties".into(), "other".into())]);
    assert_eq!(api_version(), 10);
}
