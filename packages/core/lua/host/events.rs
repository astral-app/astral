use crate::lua::enums::Event;
use crate::plugins::capability::Capability;
use core::ffi::c_int;
use std::collections::BTreeMap;
use std::sync::Mutex;

use serde_json::{Value, json};

use crate::lua::api::{self as lua, LuaState};
use crate::lua::host::{self as host, Plugin};
use crate::lua::json::{push_json, to_json};
use crate::support::host_callback::Reply;

const UTF8_CONTINUATION_MASK: u8 = 0xc0;
const UTF8_CONTINUATION: u8 = 0x80;

const fn is_utf8_continuation(byte: u8) -> bool {
    byte & UTF8_CONTINUATION_MASK == UTF8_CONTINUATION
}

fn is_snowflake(value: &str) -> bool {
    (15..=20).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn subscriptions(plugin: *mut Plugin, wants: impl Fn(&crate::lua::host::EventCallback) -> bool) -> Vec<u64> {
    unsafe { &*plugin }
        .event_callbacks
        .iter()
        .filter(|(_, callback)| wants(callback))
        .map(|(id, _)| *id)
        .collect()
}

fn reference_of(plugin: *mut Plugin, subscription: u64) -> Option<c_int> {
    let plugin = unsafe { &*plugin };
    if !plugin.running() {
        return None;
    }
    plugin.event_callbacks.get(&subscription).map(|callback| callback.reference)
}

unsafe fn set_text(state: *mut LuaState, key: &str, value: &str) {
    unsafe {
        lua::push_str(state, value);
        lua::set_field(state, -2, key);
    }
}

unsafe fn set_flag(state: *mut LuaState, key: &str, value: bool) {
    unsafe {
        lua::lua_pushboolean(state, c_int::from(value));
        lua::set_field(state, -2, key);
    }
}

pub struct MessageFacts {
    pub message_id: String,
    pub channel_id: String,
    pub author_id: String,
    pub referenced_author_id: String,
    pub guild_id: String,
    pub content: String,
    pub from_me: bool,
    pub direct: bool,
    pub open: bool,
}

fn filter_message(facts: &MessageFacts, edited: bool) -> bool {
    let _held = host::lock();
    for plugin in host::plugins() {
        if !unsafe { (*plugin).running() } {
            continue;
        }
        for subscription in subscriptions(plugin, |callback| callback.event == Event::MessageFilter) {
            let Some(reference) = reference_of(plugin, subscription) else {
                continue;
            };
            let state = unsafe { (*plugin).state() };
            unsafe {
                lua::push_reference(state, reference);
                lua::lua_createtable(state, 0, 9);
                set_text(state, "message_id", &facts.message_id);
                set_text(state, "channel_id", &facts.channel_id);
                set_text(state, "author_id", &facts.author_id);
                set_flag(state, "edited", edited);
                set_text(state, "referenced_author_id", &facts.referenced_author_id);
                set_text(state, "content", &facts.content);
                set_flag(state, "from_me", facts.from_me);
                set_flag(state, "direct", facts.direct);
                set_flag(state, "open", facts.open);
            }
            if !host::call_or_stop(plugin, 1, 1, Event::MessageFilter.value()) {
                break;
            }
            let filtered = unsafe { lua::lua_type(state, -1) == lua::TBOOLEAN && lua::lua_toboolean(state, -1) != 0 };
            unsafe { lua::pop(state, 1) };
            if filtered {
                return true;
            }
        }
    }
    false
}

#[derive(Default)]
struct Mentions {
    user: bool,
    everyone: bool,
    role: bool,
}

fn mentions_my_role(data: &Value, guild: &str, current_user: &str) -> bool {
    let Some(roles) = data
        .get("mention_roles")
        .and_then(Value::as_array)
        .filter(|roles| !roles.is_empty())
    else {
        return false;
    };
    if guild.is_empty() {
        return false;
    }
    let Some(member) = crate::discord::cache::member(guild, current_user) else {
        return false;
    };
    let Some(mine) = member.get("roles").and_then(Value::as_array) else {
        return false;
    };
    roles.iter().any(|role| role.is_string() && mine.contains(role))
}

fn emit_message_create(facts: &MessageFacts, mentions: &Mentions) {
    let _held = host::lock();
    for plugin in host::plugins() {
        if !unsafe { (*plugin).running() } {
            continue;
        }
        for subscription in subscriptions(plugin, |callback| callback.event == Event::MessageCreate) {
            let Some(reference) = reference_of(plugin, subscription) else {
                continue;
            };
            let state = unsafe { (*plugin).state() };
            unsafe {
                lua::push_reference(state, reference);
                lua::lua_createtable(state, 0, 10);
                set_text(state, "message_id", &facts.message_id);
                if !facts.guild_id.is_empty() {
                    set_text(state, "guild_id", &facts.guild_id);
                }
                set_flag(state, "everyone", mentions.everyone);
                set_flag(state, "role_mentioned", mentions.role);
                set_text(state, "channel_id", &facts.channel_id);
                set_text(state, "author_id", &facts.author_id);
                set_flag(state, "from_me", facts.from_me);
                set_flag(state, "mentioned", mentions.user);
                set_flag(state, "direct", facts.direct);
                set_flag(state, "open", facts.open);
            }
            if !host::call_or_stop(plugin, 1, 0, Event::MessageCreate.value()) {
                break;
            }
        }
    }
}

#[derive(PartialEq)]
enum Verdict {
    Keep,
    Changed,
    Drop,
}

fn run_gateway_subscribers(envelope: &mut Value) -> Verdict {
    let Some(kind) = envelope.get("t").and_then(Value::as_str).map(str::to_owned) else {
        return Verdict::Keep;
    };
    let mut verdict = Verdict::Keep;
    let _held = host::lock();
    for plugin in host::plugins() {
        if !unsafe { (*plugin).running() } {
            continue;
        }
        let wanted = subscriptions(plugin, |callback| {
            callback.event == Event::Gateway && (callback.types.is_empty() || callback.types.contains(&kind))
        });
        for subscription in wanted {
            let Some(reference) = reference_of(plugin, subscription) else {
                continue;
            };
            let state = unsafe { (*plugin).state() };
            unsafe {
                lua::push_reference(state, reference);
                lua::lua_createtable(state, 0, 4);
                set_text(state, "type", &kind);
                if let Some(sequence) = envelope.get("s").and_then(Value::as_i64) {
                    lua::lua_pushinteger(state, sequence);
                    lua::set_field(state, -2, "seq");
                }
                push_json(state, envelope.get("d").unwrap_or(&Value::Null), 0);
                lua::set_field(state, -2, "data");
            }
            if !host::call_or_stop(plugin, 1, 1, Event::Gateway.value()) {
                break;
            }
            if unsafe { lua::lua_type(state, -1) == lua::TBOOLEAN && lua::lua_toboolean(state, -1) == 0 } {
                unsafe { lua::pop(state, 1) };
                envelope["t"] = json!("ASTRAL_FILTERED");
                return Verdict::Drop;
            }
            if unsafe { lua::is_table(state, -1) }
                && let Some(replaced) = unsafe { to_json(state, -1, 0) }
            {
                envelope["d"] = replaced;
                verdict = Verdict::Changed;
            }
            unsafe { lua::pop(state, 1) };
        }
    }
    verdict
}

pub fn needs_gateway_events() -> bool {
    let _held = host::lock();
    host::plugins().into_iter().any(|plugin| {
        let plugin = unsafe { &*plugin };
        plugin.running()
            && (plugin
                .event_callbacks
                .values()
                .any(|callback| matches!(callback.event, Event::Gateway | Event::MessageCreate | Event::MessageFilter))
                || plugin.permissions.contains(&Capability::MessagesLocal))
    })
}

fn known_gateway_events(envelope: &mut Value) -> Option<String> {
    let kind = envelope.get("t").and_then(Value::as_str)?.to_owned();
    let data = envelope.get("d").filter(|data| data.is_object())?.clone();
    if kind == "READY" {
        let current = data.get("user").and_then(|user| user.get("id")).and_then(Value::as_str)?;
        if !is_snowflake(current) {
            return None;
        }
        let _held = host::lock();
        host::host().current_user_id = current.to_owned();
        return None;
    }
    let edited = kind == "MESSAGE_UPDATE";
    if kind != "MESSAGE_CREATE" && !edited {
        return None;
    }
    let channel_id = data.get("channel_id").and_then(Value::as_str)?.to_owned();
    let author_id = data
        .get("author")
        .filter(|author| author.is_object())?
        .get("id")
        .and_then(Value::as_str)?
        .to_owned();
    if !is_snowflake(&channel_id) || !is_snowflake(&author_id) {
        return None;
    }
    let message_id = data
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| is_snowflake(id))
        .unwrap_or_default()
        .to_owned();
    let content = data.get("content").and_then(Value::as_str).unwrap_or_default().to_owned();
    let current_user = {
        let _held = host::lock();
        host::host().current_user_id.clone()
    };
    let referenced_author_id = data
        .get("referenced_message")
        .and_then(|message| message.get("author"))
        .and_then(|author| author.get("id"))
        .and_then(Value::as_str)
        .filter(|id| is_snowflake(id))
        .unwrap_or_default()
        .to_owned();
    let mut mentions = Mentions::default();
    if !current_user.is_empty() {
        mentions.user = data.get("mentions").and_then(Value::as_array).is_some_and(|list| {
            list.iter()
                .any(|mention| mention.get("id").and_then(Value::as_str) == Some(current_user.as_str()))
        });
    }
    let guild_id = data.get("guild_id").and_then(Value::as_str).map(str::to_owned);
    let direct = guild_id.is_none();
    let guild_id = guild_id.unwrap_or_default();
    mentions.everyone = data.get("mention_everyone").and_then(Value::as_bool).unwrap_or(false);
    mentions.role = !current_user.is_empty() && mentions_my_role(&data, &guild_id, &current_user);
    let selected = crate::wire::ids::astral_wire_current_channel();
    let open = selected != 0 && selected.to_string() == channel_id;
    let from_me = author_id == current_user;
    let facts = MessageFacts {
        message_id,
        channel_id,
        author_id,
        referenced_author_id,
        guild_id,
        content,
        from_me,
        direct,
        open,
    };
    if edited {
        if from_me || facts.message_id.is_empty() || !filter_message(&facts, true) {
            return None;
        }
        let mut removed = json!({"id": facts.message_id, "channel_id": facts.channel_id});
        if !facts.guild_id.is_empty() {
            removed["guild_id"] = json!(facts.guild_id);
        }
        envelope["t"] = json!("MESSAGE_DELETE");
        envelope["d"] = removed;
        return Some(envelope.to_string());
    }
    if !from_me && filter_message(&facts, false) {
        envelope["t"] = json!("ASTRAL_FILTERED");
        return Some(envelope.to_string());
    }
    emit_message_create(&facts, &mentions);
    None
}

pub fn process_gateway_message(message: &[u8]) -> Option<String> {
    if message.is_empty() || message.len() > 8 << 20 {
        return None;
    }
    let mut envelope: Value = serde_json::from_slice(message).ok().filter(Value::is_object)?;
    let verdict = run_gateway_subscribers(&mut envelope);
    if verdict == Verdict::Drop {
        return Some(envelope.to_string());
    }
    if let Some(known) = known_gateway_events(&mut envelope) {
        return Some(known);
    }
    (verdict == Verdict::Changed).then(|| envelope.to_string())
}

pub struct MediaSave {
    pub url: String,
    pub kind: crate::lua::enums::MediaKind,
    pub channel_id: String,
    pub message_id: String,
}

struct PendingMediaSave {
    save: MediaSave,
    handlers: Vec<(String, usize)>,
    next: usize,
    done: Reply,
}

static MEDIA_SAVES: Mutex<(BTreeMap<u64, PendingMediaSave>, u64)> = Mutex::new((BTreeMap::new(), 1));

fn media_saves() -> std::sync::MutexGuard<'static, (BTreeMap<u64, PendingMediaSave>, u64)> {
    MEDIA_SAVES.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn finish_media_save(id: u64, url: Option<String>) {
    let pending = media_saves().0.remove(&id);
    if let Some(pending) = pending {
        (pending.done)(url);
    }
}

unsafe extern "C" fn reply_media_save(state: *mut LuaState) -> c_int {
    let id = unsafe { lua::to_integer(state, lua::upvalue_index(1)) } as u64;
    let step = unsafe { lua::to_integer(state, lua::upvalue_index(2)) } as usize;
    let url = if unsafe { lua::lua_type(state, 1) } == lua::TSTRING {
        unsafe { lua::to_bytes(state, 1) }.map(|bytes| String::from_utf8_lossy(bytes).into_owned())
    } else {
        None
    };
    let _held = host::lock();
    if media_saves().0.get(&id).is_none_or(|pending| pending.next != step) {
        return 0;
    }
    match url.filter(|url| url.len() <= 4096 && url.starts_with("https://")) {
        Some(url) => finish_media_save(id, Some(url)),
        None => advance_media_save(id),
    }
    0
}

unsafe fn push_media_save(state: *mut LuaState, save: &MediaSave) {
    unsafe {
        lua::lua_createtable(state, 0, 4);
        set_text(state, "url", &save.url);
        set_text(state, "kind", save.kind.value());
        if !save.channel_id.is_empty() {
            set_text(state, "channel_id", &save.channel_id);
        }
        if !save.message_id.is_empty() {
            set_text(state, "message_id", &save.message_id);
        }
    }
}

fn advance_media_save(id: u64) {
    loop {
        let (plugin_id, index, step, save) = {
            let mut saves = media_saves();
            let Some(pending) = saves.0.get_mut(&id) else { return };
            if pending.next >= pending.handlers.len() {
                drop(saves);
                finish_media_save(id, None);
                return;
            }
            let (plugin_id, index) = pending.handlers[pending.next].clone();
            pending.next += 1;
            let save = MediaSave {
                url: pending.save.url.clone(),
                kind: pending.save.kind,
                channel_id: pending.save.channel_id.clone(),
                message_id: pending.save.message_id.clone(),
            };
            (plugin_id, index, pending.next, save)
        };
        let Some(plugin) = host::find(&plugin_id) else { continue };
        let Some(reference) = (unsafe { &*plugin })
            .media_save_handlers
            .get(index)
            .copied()
            .filter(|_| unsafe { (*plugin).running() })
        else {
            continue;
        };
        let state = unsafe { (*plugin).state() };
        let top = unsafe { lua::lua_gettop(state) };
        unsafe {
            lua::push_reference(state, reference);
            push_media_save(state, &save);
            lua::lua_pushinteger(state, id as i64);
            lua::lua_pushinteger(state, step as i64);
            lua::lua_pushcclosure(state, reply_media_save, 2);
        }
        if !host::call(plugin, 2, 0, "media save") {
            unsafe { lua::lua_settop(state, top) };
            host::fail_and_stop(plugin);
            if media_saves().0.get(&id).is_some_and(|pending| pending.next == step) {
                continue;
            }
            return;
        }
        unsafe { lua::lua_settop(state, top) };
        return;
    }
}

pub fn has_media_save_handlers() -> bool {
    let _held = host::lock();
    host::plugins()
        .into_iter()
        .any(|plugin| unsafe { (*plugin).running() && !(*plugin).media_save_handlers.is_empty() })
}

pub fn resolve_media_save(save: MediaSave, done: Reply) -> u64 {
    let _held = host::lock();
    let mut handlers = Vec::new();
    for plugin in host::plugins() {
        let plugin = unsafe { &*plugin };
        if plugin.running() {
            handlers.extend((0..plugin.media_save_handlers.len()).map(|index| (plugin.identifier().to_owned(), index)));
        }
    }
    let id = {
        let mut saves = media_saves();
        let id = saves.1;
        saves.1 += 1;
        saves.0.insert(
            id,
            PendingMediaSave {
                save,
                handlers,
                next: 0,
                done,
            },
        );
        id
    };
    advance_media_save(id);
    id
}

pub fn cancel_media_save(id: u64) {
    let _held = host::lock();
    media_saves().0.remove(&id);
}

pub fn has_outgoing_message_transforms() -> bool {
    let _held = host::lock();
    host::plugins()
        .into_iter()
        .any(|plugin| unsafe { (*plugin).running() && !(*plugin).outgoing_message_transforms.is_empty() })
}

pub fn transform_outgoing_message(channel: &str, operation: &str, content: &str) -> Option<String> {
    if content.len() > 20000 {
        return None;
    }
    let _held = host::lock();
    let mut transformed = content.as_bytes().to_vec();
    let mut changed = false;
    for plugin in host::plugins() {
        let references = {
            let plugin = unsafe { &*plugin };
            if !plugin.running() || plugin.outgoing_message_transforms.is_empty() {
                continue;
            }
            plugin.outgoing_message_transforms.clone()
        };
        for reference in references {
            if !unsafe { (*plugin).running() } {
                break;
            }
            let state = unsafe { (*plugin).state() };
            let top = unsafe { lua::lua_gettop(state) };
            unsafe {
                lua::push_reference(state, reference);
                lua::push_bytes(state, &transformed);
                lua::lua_createtable(state, 0, 2);
                set_text(state, "channel_id", channel);
                set_text(state, "operation", operation);
            }
            if !host::call(plugin, 2, 1, "outgoing message transform") {
                unsafe { lua::lua_settop(state, top) };
                host::fail_and_stop(plugin);
                break;
            }
            if unsafe { lua::lua_type(state, -1) } == lua::TSTRING
                && let Some(value) = unsafe { lua::to_bytes(state, -1) }.filter(|value| value.len() <= 20000)
            {
                changed = changed || value != transformed.as_slice();
                transformed = value.to_vec();
            }
            unsafe { lua::lua_settop(state, top) };
        }
    }
    if !changed {
        return None;
    }
    String::from_utf8(transformed).ok()
}

const COMPOSER_STYLES: [&str; 12] = [
    "bold",
    "italic",
    "underline",
    "strike",
    "code",
    "spoiler",
    "marker",
    "heading1",
    "heading2",
    "heading3",
    "subtext",
    "quote",
];

pub fn composer_styles(text: &[u8]) -> Value {
    let mut result: Vec<Value> = Vec::new();
    if text.is_empty() || text.len() > 8192 || std::str::from_utf8(text).is_err() {
        return Value::Array(result);
    }
    let _held = host::lock();
    for plugin in host::plugins() {
        let references = {
            let plugin = unsafe { &*plugin };
            if !plugin.running() || plugin.composer_decorators.is_empty() {
                continue;
            }
            plugin.composer_decorators.clone()
        };
        for reference in references {
            let state = unsafe { (*plugin).state() };
            let top = unsafe { lua::lua_gettop(state) };
            unsafe {
                lua::push_reference(state, reference);
                lua::push_bytes(state, text);
            }
            if !host::call(plugin, 1, 1, "composer formatting") {
                unsafe { lua::lua_settop(state, top) };
                host::fail_and_stop(plugin);
                break;
            }
            if let Some(Value::Array(runs)) = unsafe { to_json(state, -1, 0) } {
                for run in runs.iter().take(128) {
                    if result.len() >= 256 {
                        break;
                    }
                    let (Some(start), Some(length), Some(style)) = (
                        run.get("start").and_then(Value::as_i64),
                        run.get("length").and_then(Value::as_i64),
                        run.get("style").and_then(Value::as_str),
                    ) else {
                        continue;
                    };
                    let size = text.len() as i64;
                    if start < 1 || length < 1 || start > size || length > size - start + 1 || !COMPOSER_STYLES.contains(&style) {
                        continue;
                    }
                    let first = (start - 1) as usize;
                    let end = first + length as usize;
                    if (first > 0 && is_utf8_continuation(text[first])) || (end < text.len() && is_utf8_continuation(text[end])) {
                        continue;
                    }
                    result.push(json!({"start": start, "length": length, "style": style}));
                }
            }
            unsafe { lua::lua_settop(state, top) };
            if result.len() >= 256 {
                return Value::Array(result);
            }
        }
    }
    Value::Array(result)
}
