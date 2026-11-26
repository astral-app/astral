use astral_core::discord::cache as discord;
use astral_core::support::sqlite_api::{self as sqlite, Database};
use serde_json::json;

const GUILD: &str = "100000000000000001";
const OWNER: &str = "200000000000000001";
const USER: &str = "200000000000000002";
const ROLE: &str = "300000000000000001";
const CHANNEL: &str = "400000000000000001";
const MESSAGE: &str = "500000000000000001";
const EMOJI: &str = "600000000000000001";

fn padded(id: &str) -> String {
    format!("{id:0>20}")
}

fn insert(database: &Database, table: &str, key: &str, value: &serde_json::Value) {
    let text = value.to_string();
    let mut statement = database.prepare(
        &format!("INSERT INTO {table} (k, v) VALUES (?, ?)"),
        &[key.as_bytes(), text.as_bytes()],
    );
    assert_eq!(statement.step(), sqlite::DONE);
}

fn build_store() {
    let directory = std::env::temp_dir().join(format!("astral-discord-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    let account = directory.join("store").join(OWNER);
    std::fs::create_dir_all(&account).unwrap();
    let database = Database::open(
        &account.join("cache.sqlite").to_string_lossy(),
        sqlite::OPEN_READWRITE | sqlite::OPEN_CREATE,
    )
    .unwrap();
    for table in [
        "guilds",
        "members",
        "users",
        "channels",
        "messages",
        "relationships",
        "channel_overwrites",
    ] {
        assert!(database.execute(&format!("CREATE TABLE {table} (k TEXT PRIMARY KEY, v BLOB)")));
    }
    insert(
        &database,
        "guilds",
        &padded(GUILD),
        &json!({"properties": {"name": "Botcave", "owner_id": OWNER}, "roles": [
            {"id": GUILD, "name": "@everyone", "permissions": "1024", "position": 0},
            {"id": ROLE, "name": "Mods", "permissions": "8192", "color": 255, "position": 1}
        ]}),
    );
    insert(
        &database,
        "members",
        &format!("{}:{}", padded(GUILD), padded(USER)),
        &json!({"roles": [ROLE], "nick": "Tester", "flags": 2}),
    );
    insert(
        &database,
        "users",
        &padded(USER),
        &json!({"username": "tester", "global_name": "Test", "email": "x"}),
    );
    insert(&database, "channels", &format!("{}:{}", padded(GUILD), padded(CHANNEL)), &json!({}));
    insert(&database, "relationships", &padded(USER), &json!({"type": 1, "nickname": "Pal"}));
    insert(
        &database,
        "channel_overwrites",
        &format!("{}:{}", padded(GUILD), padded(CHANNEL)),
        &json!([{"id": GUILD, "allow": "0", "deny": "1024"}, {"id": USER, "allow": "2048", "deny": "0"}]),
    );
    insert(
        &database,
        "messages",
        &format!("{}:{}", padded(CHANNEL), padded(MESSAGE)),
        &json!({"content": format!("hi <a:party_blob:{EMOJI}>"), "author": {"id": USER, "email": "x", "username": "tester"}, "edit_history": []}),
    );
    drop(database);
    discord::set_data_directory(&directory.to_string_lossy());
}

#[test]
fn reads_records_and_resolves_permissions() {
    build_store();
    let guild = discord::guild(GUILD).unwrap();
    assert_eq!(guild["name"], "Botcave");
    assert_eq!(guild["roles"][ROLE]["colors"]["primary_color"], 255);
    assert_eq!(discord::member(GUILD, USER).unwrap()["nick"], "Tester");
    assert_eq!(discord::member_named(GUILD, "Tester").as_deref(), Some(USER));
    assert_eq!(discord::member_named(GUILD, "Test").as_deref(), Some(USER));
    assert_eq!(discord::channel_guild(CHANNEL).as_deref(), Some(GUILD));
    assert_eq!(discord::message_channel(MESSAGE).as_deref(), Some(CHANNEL));
    assert_eq!(
        discord::user(USER).unwrap(),
        json!({"id": USER, "username": "tester", "global_name": "Test"})
    );
    assert_eq!(discord::friend_nickname(USER).as_deref(), Some("Pal"));
    assert_eq!(discord::relationship_type(USER), Some(1));
    assert_eq!(discord::current_user().as_deref(), Some(OWNER));
    assert_eq!(discord::guilds(), json!([{"id": GUILD, "name": "Botcave"}]));

    let raw = discord::raw_message(CHANNEL, MESSAGE).unwrap();
    assert!(raw.get("edit_history").is_none() && raw["author"].get("email").is_none());
    assert_eq!(discord::message(CHANNEL, MESSAGE).unwrap()["author"]["bot"], false);
    assert_eq!(
        discord::emoji(EMOJI).unwrap(),
        json!({"id": EMOJI, "name": "party_blob", "animated": true})
    );

    assert_eq!(discord::permissions(GUILD, OWNER, None), Some(u64::MAX >> 11));
    assert_eq!(discord::permissions(GUILD, USER, None), Some(1024 | 8192));
    assert_eq!(discord::permissions(GUILD, USER, Some(CHANNEL)), Some(8192 | 2048));
    assert_eq!(discord::guild("123"), None);
}
