use astral_core::network::policy::{
    is_current_user_request, is_telemetry_request, outgoing_message_target, transform_message_body, with_staff_ready, with_staff_user,
};
use serde_json::Value;

#[test]
fn blocks_telemetry_hosts_and_routes() {
    for (host, path) in [
        ("discord.com", "/api/v9/science"),
        ("discord.com", "/api/v10/metrics/v2?x=1"),
        ("DISCORD.com", "/API/v9/Science"),
        ("discord.com", "/error-reporting-proxy/ios/1/envelope/"),
        ("o64374.ingest.sentry.io", "/api/230973/envelope/"),
        ("launches.appsflyersdk.com", "/api/v6.15/iosevent"),
        ("img.litix.io", "/a.gif?x=1"),
        ("siteintercept.qualtrics.com", "/WRSiteInterceptEngine/"),
        ("measurement.app-measurement.com", "/a"),
        ("www.google-analytics.com", "/collect"),
        ("firebase.crashlytics.com", "/api/v1/"),
    ] {
        assert!(is_telemetry_request(host, path), "{host}{path}");
    }
    for (host, path) in [
        ("discord.com", "/api/v9/users/@me"),
        ("discord.com", "/api/v9/channels/1/messages"),
        ("example.com", "/api/v9/science"),
    ] {
        assert!(!is_telemetry_request(host, path), "{host}{path}");
    }
}

#[test]
fn finds_outgoing_message_targets() {
    assert_eq!(
        outgoing_message_target("discord.com", "/api/v10/channels/123/messages", "POST"),
        Some(("123".to_owned(), "create"))
    );
    assert_eq!(
        outgoing_message_target("discordapp.com", "/api/v9/channels/123/messages/456", "PATCH"),
        Some(("123".to_owned(), "edit"))
    );
    for (host, path, method) in [
        ("example.com", "/api/v10/channels/123/messages", "POST"),
        ("discord.com", "/api/v10/channels/123/messages", "GET"),
        ("discord.com", "/api/v10/channels/123/messages/456", "POST"),
        ("discord.com", "/api/v10/channels/nope/messages", "POST"),
        ("discord.com", "/api/v10/channels/123/messages/456/reactions", "PATCH"),
    ] {
        assert!(outgoing_message_target(host, path, method).is_none(), "{method} {host}{path}");
    }
}

#[test]
fn transforms_only_the_message_content() {
    let source =
        br#"{"content":"before","message_reference":{"message_id":"789"},"attachments":[{"id":"0","filename":"a.png"}],"flags":4}"#;
    let changed = transform_message_body(
        "discord.com",
        "/api/v10/channels/123/messages/456",
        "PATCH",
        source,
        |channel, operation, content| {
            assert_eq!((channel, operation, content), ("123", "edit", "before"));
            Some("after".to_owned())
        },
    )
    .expect("changed");
    let parsed: Value = serde_json::from_slice(&changed).expect("json");
    assert_eq!(parsed["content"], "after");
    assert_eq!(parsed["message_reference"]["message_id"], "789");
    assert_eq!(parsed["attachments"][0]["filename"], "a.png");
    assert_eq!(parsed["flags"], 4);
    assert!(transform_message_body("discord.com", "/api/v10/channels/123/messages", "POST", source, |_, _, _| None).is_none());
}

#[test]
fn marks_the_current_user_as_staff() {
    assert!(is_current_user_request("discord.com", "/api/v9/users/@me"));
    assert!(!is_current_user_request("discord.com", "/api/v9/users/@me/settings"));
    let user: Value = serde_json::from_slice(&with_staff_user(br#"{"id":"1","flags":64}"#).expect("user")).expect("json");
    assert_eq!((user["flags"].as_i64(), user["public_flags"].as_i64()), (Some(65), Some(1)));
    assert!(with_staff_user(b"[1]").is_none());
    let ready = with_staff_ready(br#"{"op":0,"t":"READY","d":{"user":{"id":"1","flags":0}}}"#).expect("ready");
    let ready: Value = serde_json::from_slice(&ready).expect("json");
    assert_eq!(ready["d"]["user"]["flags"], 1);
    assert!(with_staff_ready(br#"{"op":0,"t":"MESSAGE_CREATE","d":{"content":"READY"}}"#).is_none());
}
