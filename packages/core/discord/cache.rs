use core::ffi::{c_char, c_int};
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime};

use serde_json::{Map, Value, json};

use crate::support::bridge::{AstralRustString, bytes};
use crate::support::sqlite_api::{self as sqlite, Database};

const FRESHNESS: Duration = Duration::from_secs(30);
const ADMINISTRATOR: u64 = 1 << 3;
const ALL_PERMISSIONS: u64 = u64::MAX >> 11;
const RELATIONSHIP_BLOCKED: i64 = 2;
const TIMESTAMP_PREFIX: usize = 19;

#[derive(Default)]
struct State {
    data_directory: String,
    database: Option<Database>,
    cache: HashMap<String, (Instant, Value)>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn locked() -> MutexGuard<'static, Option<State>> {
    STATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn with<T>(work: impl FnOnce(&mut State) -> T) -> T {
    let mut guard = locked();
    work(guard.get_or_insert_with(State::default))
}

pub fn snowflake(value: &str) -> bool {
    (15..=20).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn key(id: &str) -> String {
    format!("{}{id}", "0".repeat(20 - id.len().min(20)))
}

fn unpadded(value: &str) -> String {
    let trimmed = value.trim_start_matches('0');
    if trimmed.is_empty() && !value.is_empty() {
        "0".into()
    } else {
        trimmed.into()
    }
}

fn first_part(value: &str) -> &str {
    value.split(':').next().unwrap_or(value)
}

fn last_part(value: &str) -> &str {
    value.rsplit(':').next().unwrap_or(value)
}

fn string_field(object: &Value, name: &str) -> Option<String> {
    object.get(name).and_then(Value::as_str).map(str::to_owned)
}

fn string_or(object: &Value, name: &str, fallback: &str) -> Value {
    Value::String(string_field(object, name).unwrap_or_else(|| fallback.to_owned()))
}

fn copy_strings(from: &Value, to: &mut Map<String, Value>, fields: &[&str]) {
    for field in fields {
        if let Some(value) = from.get(*field).filter(|value| value.is_string()) {
            to.insert((*field).to_owned(), value.clone());
        }
    }
}

fn array<'a>(object: &'a Value, name: &str) -> &'a [Value] {
    object.get(name).and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default()
}

fn number_field(object: &Value, name: &str) -> Value {
    object.get(name).filter(|value| value.is_number()).cloned().unwrap_or(json!(0))
}

fn role_colors(role: &Value) -> Value {
    let mut stops = Map::new();
    if let Some(colors) = role.get("colors").filter(|colors| colors.is_object()) {
        for name in ["primary_color", "secondary_color", "tertiary_color"] {
            if let Some(value) = colors.get(name).filter(|value| value.is_number()) {
                stops.insert(name.into(), value.clone());
            }
        }
    }
    if !stops.contains_key("primary_color")
        && let Some(color) = role.get("color").filter(|value| value.is_number())
    {
        stops.insert("primary_color".into(), color.clone());
    }
    Value::Object(stops)
}

fn bits(value: Option<&Value>) -> u64 {
    let Some(text) = value.and_then(Value::as_str) else { return 0 };
    let digits: String = text.trim_start().chars().take_while(char::is_ascii_digit).collect();
    digits.parse().unwrap_or(0)
}

fn parsed(text: Option<Vec<u8>>, object: bool) -> Option<Value> {
    let value: Value = serde_json::from_slice(&text?).ok()?;
    let shaped = if object { value.is_object() } else { value.is_array() };
    shaped.then_some(value)
}

fn newest_store(directory: &str) -> Option<PathBuf> {
    let mut newest: Option<(SystemTime, PathBuf)> = None;
    for account in std::fs::read_dir(PathBuf::from(directory).join("store")).ok()?.flatten() {
        let path = account.path().join("cache.sqlite");
        let Ok(modified) = std::fs::metadata(&path).and_then(|metadata| metadata.modified()) else {
            continue;
        };
        if newest.as_ref().is_none_or(|(time, _)| modified > *time) {
            newest = Some((modified, path));
        }
    }
    newest.map(|(_, path)| path)
}

impl State {
    fn database(&mut self) -> Option<&Database> {
        if self.database.is_none() {
            let path = newest_store(&self.data_directory)?;
            let uri = format!("file:{}?mode=ro", path.display());
            let database = Database::open(&uri, sqlite::OPEN_READONLY | sqlite::OPEN_URI | sqlite::OPEN_NOMUTEX)?;
            database.busy_timeout(50);
            self.cache.clear();
            self.database = Some(database);
        }
        self.database.as_ref()
    }

    fn cached(&self, key: &str) -> Option<&Value> {
        self.cache
            .get(key)
            .filter(|(time, _)| time.elapsed() <= FRESHNESS)
            .map(|(_, value)| value)
    }

    fn remember(&mut self, key: String, value: Option<Value>) -> Option<Value> {
        self.cache.insert(key, (Instant::now(), value.clone().unwrap_or(Value::Null)));
        value
    }

    fn hit(&self, key: &str) -> Option<Option<Value>> {
        self.cached(key).map(|value| (!value.is_null()).then(|| value.clone()))
    }

    fn text_hit(&self, key: &str) -> Option<Option<String>> {
        self.cached(key).map(|value| value.as_str().map(str::to_owned))
    }

    fn remember_text(&mut self, key: String, value: Option<String>) -> Option<String> {
        self.remember(key, value.clone().map(Value::String));
        value
    }

    fn column(&mut self, sql: &str, values: &[&[u8]]) -> Option<Vec<u8>> {
        let mut statement = self.database()?.prepare(sql, values);
        if statement.step() != sqlite::ROW {
            return None;
        }
        Some(statement.blob(0)).filter(|blob| !blob.is_empty())
    }

    fn column_text(&mut self, sql: &str, argument: &str) -> Option<String> {
        self.column(sql, &[argument.as_bytes()])
            .map(|blob| String::from_utf8_lossy(&blob).into_owned())
    }

    fn record(&mut self, sql: &str, argument: &str, object: bool) -> Option<Value> {
        parsed(self.column(sql, &[argument.as_bytes()]), object)
    }

    fn set_data_directory(&mut self, directory: &str) {
        if self.data_directory == directory {
            return;
        }
        self.data_directory = directory.to_owned();
        self.database = None;
        self.cache.clear();
    }

    fn guild(&mut self, guild_id: &str) -> Option<Value> {
        if !snowflake(guild_id) {
            return None;
        }
        let cache_key = format!("guild:{guild_id}");
        if let Some(hit) = self.hit(&cache_key) {
            return hit;
        }
        let record = self.record("SELECT v FROM guilds WHERE k = ?", &key(guild_id), true);
        let Some(properties) = record
            .as_ref()
            .and_then(|record| record.get("properties"))
            .filter(|value| value.is_object())
        else {
            return self.remember(cache_key, None);
        };
        let record = record.as_ref().unwrap_or(&Value::Null);
        let mut roles = Map::new();
        for role in array(record, "roles") {
            let Some(id) = role.get("id").and_then(Value::as_str) else {
                continue;
            };
            let mut entry = Map::new();
            entry.insert("id".into(), json!(id));
            entry.insert("name".into(), string_or(role, "name", ""));
            entry.insert("permissions".into(), string_or(role, "permissions", "0"));
            entry.insert("color".into(), number_field(role, "color"));
            entry.insert("position".into(), number_field(role, "position"));
            entry.insert("colors".into(), role_colors(role));
            copy_strings(role, &mut entry, &["icon", "unicode_emoji"]);
            roles.insert(id.to_owned(), Value::Object(entry));
        }
        let found = json!({
            "id": guild_id,
            "name": string_or(properties, "name", ""),
            "owner_id": string_or(properties, "owner_id", ""),
            "roles": roles,
        });
        self.remember(cache_key, Some(found))
    }

    fn member(&mut self, guild_id: &str, user_id: &str) -> Option<Value> {
        if !snowflake(guild_id) || !snowflake(user_id) {
            return None;
        }
        let cache_key = format!("member:{guild_id}:{user_id}");
        if let Some(hit) = self.hit(&cache_key) {
            return hit;
        }
        let Some(record) = self.record(
            "SELECT v FROM members WHERE k = ?",
            &format!("{}:{}", key(guild_id), key(user_id)),
            true,
        ) else {
            return self.remember(cache_key, None);
        };
        let roles: Vec<Value> = array(&record, "roles").iter().filter(|role| role.is_string()).cloned().collect();
        let mut found = Map::new();
        found.insert("user_id".into(), json!(user_id));
        found.insert("roles".into(), Value::Array(roles));
        found.insert("nick".into(), string_or(&record, "nick", ""));
        copy_strings(&record, &mut found, &["joined_at"]);
        if let Some(flags) = record.get("flags").filter(|flags| flags.is_i64() || flags.is_u64()) {
            found.insert("flags".into(), flags.clone());
        }
        self.remember(cache_key, Some(Value::Object(found)))
    }

    fn member_where(&mut self, sql: &str, guild_key: &str, name: &str) -> Option<String> {
        let pattern = format!("{guild_key}:%");
        let mut statement = self.database()?.prepare(sql, &[pattern.as_bytes(), name.as_bytes()]);
        if statement.step() != sqlite::ROW {
            return None;
        }
        let found = String::from_utf8_lossy(&statement.text(0)).into_owned();
        Some(unpadded(last_part(&found))).filter(|user| snowflake(user))
    }

    fn member_named(&mut self, guild_id: &str, name: &str) -> Option<String> {
        if !snowflake(guild_id) || name.is_empty() {
            return None;
        }
        let cache_key = format!("member-named:{guild_id}:{name}");
        if let Some(hit) = self.text_hit(&cache_key) {
            return hit;
        }
        let guild_key = key(guild_id);
        let user = self
            .member_where(
                "SELECT k FROM members WHERE k LIKE ? AND json_extract(v, '$.nick') = ? LIMIT 1",
                &guild_key,
                name,
            )
            .or_else(|| {
                self.member_where(
                    "SELECT m.k FROM members m JOIN users u ON u.k = substr(m.k, 22) WHERE m.k LIKE ? AND \
                     (json_extract(u.v, '$.global_name') = ?2 OR json_extract(u.v, '$.username') = ?2) LIMIT 1",
                    &guild_key,
                    name,
                )
            });
        self.remember_text(cache_key, user)
    }

    fn owner_of(&mut self, cache_key: String, sql: &str, id: &str) -> Option<String> {
        if !snowflake(id) {
            return None;
        }
        if let Some(hit) = self.text_hit(&cache_key) {
            return hit;
        }
        let owner = self
            .column_text(sql, &key(id))
            .map(|row| unpadded(first_part(&row)))
            .filter(|owner| snowflake(owner));
        self.remember_text(cache_key, owner)
    }

    fn channel_guild(&mut self, channel_id: &str) -> Option<String> {
        self.owner_of(
            format!("channel:{channel_id}"),
            "SELECT k FROM channels WHERE k LIKE '%:' || ?",
            channel_id,
        )
    }

    fn message_channel(&mut self, message_id: &str) -> Option<String> {
        self.owner_of(
            format!("message:{message_id}"),
            "SELECT k FROM messages WHERE k LIKE '%:' || ?",
            message_id,
        )
    }

    fn user(&mut self, user_id: &str) -> Option<Value> {
        if !snowflake(user_id) {
            return None;
        }
        let cache_key = format!("user:{user_id}");
        if let Some(hit) = self.hit(&cache_key) {
            return hit;
        }
        let Some(record) = self.record("SELECT v FROM users WHERE k = ?", &key(user_id), true) else {
            return self.remember(cache_key, None);
        };
        let mut found = Map::new();
        found.insert("id".into(), json!(user_id));
        copy_strings(&record, &mut found, &["avatar", "username", "global_name"]);
        self.remember(cache_key, Some(Value::Object(found)))
    }

    fn friend_nickname(&mut self, user_id: &str) -> Option<String> {
        if !snowflake(user_id) {
            return None;
        }
        let cache_key = format!("relationship:{user_id}");
        if let Some(hit) = self.text_hit(&cache_key) {
            return hit;
        }
        let nickname = self
            .record("SELECT v FROM relationships WHERE k = ?", &key(user_id), true)
            .and_then(|record| string_field(&record, "nickname"))
            .filter(|nickname| !nickname.is_empty());
        self.remember_text(cache_key, nickname)
    }

    fn relationship_type(&mut self, user_id: &str) -> Option<i64> {
        if !snowflake(user_id) {
            return None;
        }
        self.record("SELECT v FROM relationships WHERE k = ?", &key(user_id), true)?
            .get("type")?
            .as_i64()
    }

    fn user_ignored(&mut self, user_id: &str) -> bool {
        if !snowflake(user_id) {
            return false;
        }
        self.record("SELECT v FROM relationships WHERE k = ?", &key(user_id), true)
            .is_some_and(|record| {
                record.get("user_ignored").and_then(Value::as_bool) == Some(true)
                    && record.get("type").and_then(Value::as_i64) != Some(RELATIONSHIP_BLOCKED)
            })
    }

    fn channel_muted(&mut self, channel_id: &str, now: &str) -> bool {
        if !snowflake(channel_id) {
            return false;
        }
        let guild = self.channel_guild(channel_id).map_or_else(|| key(""), |guild| key(&guild));
        let Some(settings) = self.record("SELECT v FROM user_guild_settings WHERE k = ?", &guild, true) else {
            return false;
        };
        muted_now(&settings, now)
            || array(&settings, "channel_overrides")
                .iter()
                .find(|entry| entry.get("channel_id").and_then(Value::as_str) == Some(channel_id))
                .is_some_and(|entry| muted_now(entry, now))
    }

    fn message_containing(&mut self, text: &str) -> Option<(String, String)> {
        if !(16..=4096).contains(&text.len()) {
            return None;
        }
        let row = self.column_text("SELECT k FROM messages WHERE instr(v, ?) > 0 ORDER BY k DESC LIMIT 1", text)?;
        let (channel, message) = row.split_once(':')?;
        let (channel, message) = (unpadded(channel), unpadded(message));
        (snowflake(&channel) && snowflake(&message)).then_some((channel, message))
    }

    fn message_record(&mut self, channel_id: &str, message_id: &str) -> Option<Value> {
        if !snowflake(channel_id) || !snowflake(message_id) {
            return None;
        }
        self.record(
            "SELECT v FROM messages WHERE k = ?",
            &format!("{}:{}", key(channel_id), key(message_id)),
            true,
        )
    }

    fn raw_message(&mut self, channel_id: &str, message_id: &str) -> Option<Value> {
        let mut record = self.message_record(channel_id, message_id)?;
        if let Some(author) = record.get_mut("author").and_then(Value::as_object_mut) {
            for field in ["email", "phone", "mfa_enabled", "verified", "personal_connection_id"] {
                author.remove(field);
            }
        }
        if let Some(object) = record.as_object_mut() {
            for field in ["edit_history", "deleted", "first_edit_timestamp"] {
                object.remove(field);
            }
        }
        Some(record)
    }

    fn emoji(&mut self, emoji_id: &str) -> Option<Value> {
        if !snowflake(emoji_id) {
            return None;
        }
        let cache_key = format!("emoji:{emoji_id}");
        if let Some(hit) = self.hit(&cache_key) {
            return hit;
        }
        let described = self
            .message_containing(emoji_id)
            .and_then(|(channel, message)| self.raw_message(&channel, &message))
            .and_then(|record| {
                emoji_in_content(record.get("content").and_then(Value::as_str).unwrap_or_default(), emoji_id)
                    .or_else(|| emoji_in_reactions(&record, emoji_id))
            });
        self.remember(cache_key, described)
    }

    fn current_user(&self) -> Option<String> {
        let path = newest_store(&self.data_directory)?;
        let account = path.parent()?.file_name()?.to_string_lossy().into_owned();
        snowflake(&account).then_some(account)
    }

    fn guilds(&mut self) -> Value {
        let ids: Vec<String> = {
            let Some(database) = self.database() else { return json!([]) };
            let mut statement = database.prepare("SELECT k FROM guilds", &[]);
            let mut ids = Vec::new();
            while statement.step() == sqlite::ROW {
                let text = statement.text(0);
                if !text.is_empty() {
                    ids.push(unpadded(&String::from_utf8_lossy(&text)));
                }
            }
            ids
        };
        let mut list = Vec::new();
        for id in ids {
            if list.len() >= 512 {
                break;
            }
            if let Some(found) = self.guild(&id) {
                list.push(json!({"id": found["id"], "name": found["name"]}));
            }
        }
        Value::Array(list)
    }

    fn message(&mut self, channel_id: &str, message_id: &str) -> Option<Value> {
        if !snowflake(channel_id) || !snowflake(message_id) {
            return None;
        }
        let cache_key = format!("message-record:{channel_id}:{message_id}");
        if let Some(hit) = self.hit(&cache_key) {
            return hit;
        }
        let record = self.message_record(channel_id, message_id)?;
        let author = record
            .get("author")
            .filter(|author| author.is_object())
            .cloned()
            .unwrap_or_else(|| json!({}));
        let mut found = Map::new();
        found.insert("id".into(), json!(message_id));
        found.insert("channel_id".into(), json!(channel_id));
        copy_strings(&record, &mut found, &["content", "webhook_id"]);
        let mut author_record = Map::new();
        author_record.insert("id".into(), string_or(&author, "id", ""));
        author_record.insert("bot".into(), json!(author.get("bot").and_then(Value::as_bool).unwrap_or(false)));
        copy_strings(&author, &mut author_record, &["username", "global_name"]);
        found.insert("author".into(), Value::Object(author_record));
        if let Some(kind) = record.get("type").filter(|kind| kind.is_number()) {
            found.insert("type".into(), kind.clone());
        }
        found.insert("flags".into(), json!(record.get("flags").and_then(Value::as_u64).unwrap_or(0)));
        found.insert("embeds".into(), json!(array(&record, "embeds").len()));
        found.insert(
            "reactions".into(),
            Value::Array(array(&record, "reactions").iter().filter_map(reaction_summary).collect()),
        );
        let mentions: Vec<Value> = array(&record, "mentions")
            .iter()
            .filter_map(|mentioned| {
                let id = mentioned.get("id").filter(|id| id.is_string())?;
                let mut entry = Map::new();
                entry.insert("id".into(), id.clone());
                copy_strings(mentioned, &mut entry, &["username", "global_name", "avatar"]);
                if let Some(nick) = mentioned
                    .get("member")
                    .and_then(|member| member.get("nick"))
                    .filter(|nick| nick.is_string())
                {
                    entry.insert("nick".into(), nick.clone());
                }
                Some(Value::Object(entry))
            })
            .collect();
        found.insert("mentions".into(), Value::Array(mentions));
        let roles: Vec<Value> = array(&record, "mention_roles")
            .iter()
            .filter(|role| role.is_string())
            .cloned()
            .collect();
        found.insert("mention_roles".into(), Value::Array(roles));
        if let Some(reference) = record.get("message_reference")
            && let (Some(message), Some(channel)) = (
                reference.get("message_id").filter(|id| id.is_string()),
                reference.get("channel_id").filter(|id| id.is_string()),
            )
        {
            found.insert("reference".into(), json!({"message_id": message, "channel_id": channel}));
        }
        self.remember(cache_key, Some(Value::Object(found)))
    }

    fn overwrites(&mut self, guild_id: &str, channel_id: &str) -> Option<Value> {
        let cache_key = format!("overwrites:{guild_id}:{channel_id}");
        if let Some(hit) = self.hit(&cache_key) {
            return hit;
        }
        let list = self.record(
            "SELECT v FROM channel_overwrites WHERE k = ?",
            &format!("{}:{}", key(guild_id), key(channel_id)),
            false,
        );
        self.remember(cache_key, list)
    }

    fn permissions(&mut self, guild_id: &str, user_id: &str, channel_id: Option<&str>) -> Option<u64> {
        let server = self.guild(guild_id);
        let membership = self.member(guild_id, user_id);
        let server = server?;
        if server["owner_id"].as_str() == Some(user_id) {
            return Some(ALL_PERMISSIONS);
        }
        let membership = membership?;
        let roles = &server["roles"];
        let role_permissions = |role: &str| bits(roles.get(role).and_then(|role| role.get("permissions")));
        let mut granted = role_permissions(guild_id);
        for role in array(&membership, "roles").iter().filter_map(Value::as_str) {
            granted |= role_permissions(role);
        }
        if granted & ADMINISTRATOR != 0 {
            return Some(ALL_PERMISSIONS);
        }
        if let Some(channel_id) = channel_id.filter(|channel| snowflake(channel))
            && let Some(list) = self.overwrites(guild_id, channel_id)
        {
            granted = apply_overwrites(granted, &list, guild_id, &membership, user_id);
        }
        Some(granted)
    }
}

fn apply_overwrites(mut permissions: u64, list: &Value, guild_id: &str, member: &Value, user_id: &str) -> u64 {
    let roles: BTreeSet<&str> = array(member, "roles").iter().filter_map(Value::as_str).collect();
    let overwrites = list.as_array().map(Vec::as_slice).unwrap_or_default();
    let (mut role_allow, mut role_deny) = (0, 0);
    for overwrite in overwrites.iter().filter(|overwrite| overwrite.is_object()) {
        let id = overwrite.get("id").and_then(Value::as_str).unwrap_or_default();
        if id == user_id {
            continue;
        }
        if id == guild_id {
            permissions = (permissions & !bits(overwrite.get("deny"))) | bits(overwrite.get("allow"));
        } else if roles.contains(id) {
            role_allow |= bits(overwrite.get("allow"));
            role_deny |= bits(overwrite.get("deny"));
        }
    }
    permissions = (permissions & !role_deny) | role_allow;
    for overwrite in overwrites
        .iter()
        .filter(|overwrite| overwrite.get("id").and_then(Value::as_str) == Some(user_id))
    {
        permissions = (permissions & !bits(overwrite.get("deny"))) | bits(overwrite.get("allow"));
    }
    permissions
}

fn muted_now(setting: &Value, now: &str) -> bool {
    if setting.get("muted").and_then(Value::as_bool) != Some(true) {
        return false;
    }
    let end = setting
        .get("mute_config")
        .and_then(|config| config.get("end_time"))
        .and_then(Value::as_str);
    end.is_none_or(|end| end.get(..TIMESTAMP_PREFIX).unwrap_or(end) > now.get(..TIMESTAMP_PREFIX).unwrap_or(now))
}

fn emoji_in_content(content: &str, emoji_id: &str) -> Option<Value> {
    let end = content.find(&format!(":{emoji_id}>"))?;
    let open = content[..end].rfind('<')?;
    let inner = &content[open + 1..end];
    let animated = inner.starts_with("a:");
    let name = inner.split_once(':').map_or(inner, |(_, name)| name);
    let valid = (2..=32).contains(&name.len()) && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
    valid.then(|| json!({"id": emoji_id, "name": name, "animated": animated}))
}

fn emoji_in_reactions(record: &Value, emoji_id: &str) -> Option<Value> {
    let emoji = array(record, "reactions")
        .iter()
        .filter_map(|reaction| reaction.get("emoji").filter(|emoji| emoji.is_object()))
        .find(|emoji| emoji.get("id").and_then(Value::as_str) == Some(emoji_id))?;
    let name = string_field(emoji, "name").filter(|name| !name.is_empty())?;
    Some(json!({"id": emoji_id, "name": name, "animated": emoji.get("animated").and_then(Value::as_bool).unwrap_or(false)}))
}

pub fn set_data_directory(directory: &str) {
    with(|state| state.set_data_directory(directory));
}

pub fn guild(guild_id: &str) -> Option<Value> {
    with(|state| state.guild(guild_id))
}

pub fn member(guild_id: &str, user_id: &str) -> Option<Value> {
    with(|state| state.member(guild_id, user_id))
}

pub fn member_named(guild_id: &str, name: &str) -> Option<String> {
    with(|state| state.member_named(guild_id, name))
}

pub fn channel_guild(channel_id: &str) -> Option<String> {
    with(|state| state.channel_guild(channel_id))
}

pub fn user(user_id: &str) -> Option<Value> {
    with(|state| state.user(user_id))
}

pub fn friend_nickname(user_id: &str) -> Option<String> {
    with(|state| state.friend_nickname(user_id))
}

pub fn user_ignored(user_id: &str) -> bool {
    with(|state| state.user_ignored(user_id))
}

pub fn channel_muted(channel_id: &str, now: &str) -> bool {
    with(|state| state.channel_muted(channel_id, now))
}

pub fn relationship_type(user_id: &str) -> Option<i64> {
    with(|state| state.relationship_type(user_id))
}

pub fn message_channel(message_id: &str) -> Option<String> {
    with(|state| state.message_channel(message_id))
}

pub fn message(channel_id: &str, message_id: &str) -> Option<Value> {
    with(|state| state.message(channel_id, message_id))
}

pub fn message_containing(text: &str) -> Option<(String, String)> {
    with(|state| state.message_containing(text))
}

pub fn raw_message(channel_id: &str, message_id: &str) -> Option<Value> {
    with(|state| state.raw_message(channel_id, message_id))
}

pub fn emoji(emoji_id: &str) -> Option<Value> {
    with(|state| state.emoji(emoji_id))
}

pub fn guilds() -> Value {
    with(State::guilds)
}

pub fn current_user() -> Option<String> {
    with(|state| state.current_user())
}

pub fn permissions(guild_id: &str, user_id: &str, channel_id: Option<&str>) -> Option<u64> {
    with(|state| state.permissions(guild_id, user_id, channel_id))
}

fn text(data: *const c_char, length: usize) -> String {
    String::from_utf8_lossy(bytes(data, length)).into_owned()
}

fn none() -> AstralRustString {
    AstralRustString {
        bytes: core::ptr::null_mut(),
        length: 0,
    }
}

fn json_result(value: Option<Value>) -> AstralRustString {
    value.map_or_else(none, |value| AstralRustString::new(value.to_string().into_bytes()))
}

fn text_result(value: Option<String>) -> AstralRustString {
    value.map_or_else(none, |value| AstralRustString::new(value.into_bytes()))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_discord_set_data_directory(directory: *const c_char, length: usize) {
    set_data_directory(&text(directory, length));
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_discord_guild(id: *const c_char, length: usize) -> AstralRustString {
    json_result(guild(&text(id, length)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_discord_member(
    guild_id: *const c_char,
    guild_length: usize,
    user_id: *const c_char,
    user_length: usize,
) -> AstralRustString {
    json_result(member(&text(guild_id, guild_length), &text(user_id, user_length)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_discord_member_named(
    guild_id: *const c_char,
    guild_length: usize,
    name: *const c_char,
    name_length: usize,
) -> AstralRustString {
    text_result(member_named(&text(guild_id, guild_length), &text(name, name_length)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_discord_channel_guild(id: *const c_char, length: usize) -> AstralRustString {
    text_result(channel_guild(&text(id, length)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_discord_user(id: *const c_char, length: usize) -> AstralRustString {
    json_result(user(&text(id, length)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_discord_friend_nickname(id: *const c_char, length: usize) -> AstralRustString {
    text_result(friend_nickname(&text(id, length)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_discord_relationship_type(id: *const c_char, length: usize, found: *mut c_int) -> c_int {
    let kind = relationship_type(&text(id, length));
    if let Some(found) = unsafe { found.as_mut() } {
        *found = c_int::from(kind.is_some());
    }
    kind.and_then(|kind| c_int::try_from(kind).ok()).unwrap_or(0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_discord_message_channel(id: *const c_char, length: usize) -> AstralRustString {
    text_result(message_channel(&text(id, length)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_discord_message(
    channel_id: *const c_char,
    channel_length: usize,
    message_id: *const c_char,
    message_length: usize,
) -> AstralRustString {
    json_result(message(&text(channel_id, channel_length), &text(message_id, message_length)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_discord_message_containing(needle: *const c_char, length: usize) -> AstralRustString {
    message_containing(&text(needle, length)).map_or_else(none, |(channel, message)| {
        AstralRustString::frames(&[channel.into_bytes(), message.into_bytes()])
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_discord_raw_message(
    channel_id: *const c_char,
    channel_length: usize,
    message_id: *const c_char,
    message_length: usize,
) -> AstralRustString {
    json_result(raw_message(&text(channel_id, channel_length), &text(message_id, message_length)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_discord_emoji(id: *const c_char, length: usize) -> AstralRustString {
    json_result(emoji(&text(id, length)))
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_discord_guilds() -> AstralRustString {
    AstralRustString::new(guilds().to_string().into_bytes())
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_discord_current_user() -> AstralRustString {
    text_result(current_user())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_discord_permissions(
    guild_id: *const c_char,
    guild_length: usize,
    user_id: *const c_char,
    user_length: usize,
    channel_id: *const c_char,
    channel_length: usize,
    granted: *mut u64,
) -> c_int {
    let channel = (!channel_id.is_null()).then(|| text(channel_id, channel_length));
    let Some(permissions) = permissions(&text(guild_id, guild_length), &text(user_id, user_length), channel.as_deref()) else {
        return 0;
    };
    if let Some(granted) = unsafe { granted.as_mut() } {
        *granted = permissions;
    }
    1
}

fn reaction_summary(reaction: &Value) -> Option<Value> {
    let emoji = reaction.get("emoji")?;
    let mut summary = Map::new();
    if let Some(id) = emoji.get("id").filter(|id| id.is_string()) {
        summary.insert("emoji_id".into(), id.clone());
    }
    if let Some(name) = emoji.get("name").filter(|name| name.is_string()) {
        summary.insert("emoji_name".into(), name.clone());
    }
    let details = reaction.get("count_details");
    let number = |value: Option<&Value>| value.and_then(Value::as_u64).unwrap_or(0);
    let normal = details.and_then(|details| details.get("normal")).or_else(|| reaction.get("count"));
    summary.insert("count".into(), json!(number(normal)));
    summary.insert(
        "burst_count".into(),
        json!(number(details.and_then(|details| details.get("burst")))),
    );
    summary.insert("me".into(), json!(reaction.get("me").and_then(Value::as_bool).unwrap_or(false)));
    Some(Value::Object(summary))
}
