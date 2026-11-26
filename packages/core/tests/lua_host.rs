mod support;

use core::ffi::{CStr, c_char};
use std::path::Path;
use std::sync::{Arc, Mutex};

use astral_core::lua::host::http::{HttpRequest, HttpResponse};
use astral_core::lua::host::{self as lua_host, events, http, nodes, ui};
use astral_core::wire::abi::{AstralNodePatch, AstralPatchTarget, PATCH_BACKGROUND, PATCH_HIDDEN, PATCH_PADDING, PATCH_RADIUS};
use astral_core::wire::decorate::{AstralDecorationContext, AstralMessageBackgroundContext, AstralMessageBackgroundStyle, AstralTag};
use astral_core::wire::insert::{AstralInsertNode, INSERT_CAPACITY, INSERT_TEXT};
use astral_core::wire::message_menu::AstralMenuEntry;
use serde_json::{Value, json};
use support::{developer_settings, eval, on, recorded, recorder, start_host, stop_host, write};

const INSERT_VIEW: i32 = 2;
const INSERT_ROW: u32 = 1 << 8;
const FILE_LIMIT: usize = 1 << 20;
const COMPOSER_LIMIT: usize = 8193;
const HOLD_ATTEMPTS: usize = 200;
const HOLD_POLL: std::time::Duration = std::time::Duration::from_millis(10);

fn plugin(root: &Path, folder: &str, manifest: &str, main: &str) {
    write(&root.join("plugins").join(folder).join("manifest.toml"), manifest);
    write(&root.join("plugins").join(folder).join("main.lua"), main);
}

fn manifest(identifier: &str, permissions: &[&str]) -> String {
    let permissions: Vec<String> = permissions.iter().map(|permission| format!("'{permission}'")).collect();
    format!(
        "id = '{identifier}'\nname = '{identifier}'\nversion = '1.0'\nmain = 'main.lua'\npermissions = [{}]\n",
        permissions.join(", ")
    )
}

fn c_text(text: &[c_char]) -> String {
    unsafe { CStr::from_ptr(text.as_ptr()) }.to_string_lossy().into_owned()
}

fn number(text: &str) -> u64 {
    text.parse().unwrap_or_else(|_| panic!("not a request id: {text}"))
}

fn last<T: Clone>(record: &Arc<Mutex<Vec<T>>>) -> Option<T> {
    recorded(record).last().cloned()
}

fn push<T: Send + 'static>(record: &Arc<Mutex<Vec<T>>>, value: T) {
    record.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push(value);
}

fn network_requests(work: &Path) {
    let root = work.join("network-host");
    plugin(
        &root,
        "network",
        &manifest("astral.network-test", &["network", "storage", "discord.api"]),
        "local request = astral.net.request({ url = 'https://example.com/api', method = 'patch', timeout_ms = 30000, headers = { accept = 'application/json' }, body = 'request body' }, function(response) astral.storage.set('response', response) end)\nastral.storage.set('initial', request)\n",
    );
    plugin(
        &root,
        "restricted",
        &manifest("astral.network-restricted", &["storage"]),
        "return true",
    );
    let (submitted, cancelled) = (recorder::<(u64, Value)>(), recorder::<u64>());
    let record = submitted.clone();
    on("networkRequest", move |arguments| {
        push(&record, (arguments[0].as_u64().unwrap_or(0), arguments[1].clone()));
        Value::Null
    });
    let record = cancelled.clone();
    on("cancelNetworkRequest", move |arguments| {
        push(&record, arguments[0].as_u64().unwrap_or(0));
        Value::Null
    });
    let discord = recorder::<(u64, Value)>();
    let record = discord.clone();
    on("discordRequest", move |arguments| {
        push(&record, (arguments[0].as_u64().unwrap_or(0), arguments[1].clone()));
        Value::Null
    });
    start_host(&root, developer_settings(&root), None);
    let (first, options) = last(&submitted).expect("submitted");
    assert!(first > 0);
    assert_eq!(
        (options["method"].as_str(), options["timeout_ms"].as_i64()),
        (Some("PATCH"), Some(30_000))
    );
    assert_eq!(
        (options["body"].as_str(), options["headers"]["accept"].as_str()),
        (Some("request body"), Some("application/json"))
    );
    assert_eq!(
        eval("astral.network-test", "return astral.storage.get('initial')"),
        first.to_string()
    );
    lua_host::complete_network_request(
        first,
        &json!({"status": 201, "ok": true, "url": "https://example.com/api", "headers": {"content-type": "application/json"}, "body": r#"{"result":{"translatedText":"complete"}}"#}),
    );
    assert_eq!(
        eval(
            "astral.network-test",
            "return astral.storage.get('response').json.result.translatedText"
        ),
        "complete"
    );
    let invalid = number(&eval(
        "astral.network-test",
        "return astral.net.request({ url = 'https://example.com/invalid-json' }, function(response) astral.storage.set('invalid_json', response.json) end)",
    ));
    lua_host::complete_network_request(invalid, &json!({"status": 200, "ok": true, "body": "not json"}));
    assert_eq!(
        eval("astral.network-test", "return astral.storage.get('invalid_json') == nil"),
        "true"
    );
    let cancelled_request = number(&eval(
        "astral.network-test",
        "return astral.net.request({ url = 'https://example.com/cancel' }, function() astral.storage.set('cancelled_callback', true) end)",
    ));
    assert_eq!(
        eval("astral.network-test", &format!("return astral.net.cancel({cancelled_request})")),
        "true"
    );
    assert_eq!(last(&cancelled), Some(cancelled_request));
    lua_host::complete_network_request(cancelled_request, &json!({"status": 200, "ok": true, "body": "ignored"}));
    assert_eq!(
        eval("astral.network-test", "return astral.storage.get('cancelled_callback') == nil"),
        "true"
    );
    let before = recorded(&submitted).len();
    assert_eq!(
        eval(
            "astral.network-test",
            "local ok = pcall(function() astral.net.request({ url = 'http://example.com' }, function() end) end); return ok"
        ),
        "false"
    );
    assert_eq!(
        eval(
            "astral.network-restricted",
            "local ok = pcall(function() astral.net.request({ url = 'https://example.com' }, function() end) end); return ok"
        ),
        "false"
    );
    assert_eq!(
        eval(
            "astral.network-test",
            "local ok = pcall(function() astral.net.request({ url = 'https://discord.com/api/v9/users/@me' }, function() end) end); return ok"
        ),
        "false"
    );
    assert_eq!(recorded(&submitted).len(), before);
    let edit = number(&eval(
        "astral.network-test",
        "return astral.discord.request({ method = 'patch', path = '/channels/1/messages/2', body = { flags = 4 } }, function(response) astral.storage.set('edited', response.status) end)",
    ));
    let (sent, options) = last(&discord).expect("discord request");
    assert_eq!(sent, edit);
    assert_eq!(
        (options["method"].as_str(), options["path"].as_str(), options["body"].as_str()),
        (Some("PATCH"), Some("/channels/1/messages/2"), Some(r#"{"flags":4}"#))
    );
    lua_host::complete_network_request(edit, &json!({"status": 200, "ok": true, "body": "{}"}));
    assert_eq!(eval("astral.network-test", "return astral.storage.get('edited')"), "200");
    assert_eq!(
        eval(
            "astral.network-test",
            "local ok = pcall(function() astral.discord.request({ path = 'https://evil.example/x' }, function() end) end); return ok"
        ),
        "false"
    );
    let stopped = number(&eval(
        "astral.network-test",
        "return astral.net.request({ url = 'https://example.com/stop' }, function() astral.storage.set('stopped_callback', true) end)",
    ));
    stop_host();
    assert_eq!(last(&cancelled), Some(stopped));
    lua_host::complete_network_request(stopped, &json!({"status": 200, "ok": true, "body": "ignored"}));
}

fn outgoing_message_transforms(work: &Path) {
    let root = work.join("message-transform-host");
    plugin(
        &root,
        "first",
        &manifest("astral.transform-first", &["discord.messages.send", "storage"]),
        "astral.messages.transform(function(content, context)\n  astral.storage.set('context', {channel = context.channel_id, operation = context.operation})\n  if context.channel_id == '100' then return content:gsub('foo', 'bar') end\nend)\n",
    );
    plugin(
        &root,
        "second",
        &manifest("astral.transform-second", &["discord.messages.send"]),
        "astral.messages.transform(function(content) return content .. '!' end)\n",
    );
    plugin(
        &root,
        "restricted",
        &manifest("astral.transform-restricted", &["storage"]),
        "local ok = pcall(function() astral.messages.transform(function(content) return content end) end)\nastral.storage.set('allowed', ok)\n",
    );
    start_host(&root, developer_settings(&root), None);
    assert!(events::has_outgoing_message_transforms());
    assert_eq!(
        events::transform_outgoing_message("100", "create", "foo message").as_deref(),
        Some("bar message!")
    );
    assert_eq!(
        eval(
            "astral.transform-first",
            "return astral.storage.get('context').channel .. ':' .. astral.storage.get('context').operation"
        ),
        "100:create"
    );
    assert_eq!(
        events::transform_outgoing_message("200", "edit", "foo message").as_deref(),
        Some("foo message!")
    );
    assert_eq!(eval("astral.transform-restricted", "return astral.storage.get('allowed')"), "false");
    lua_host::set_enabled("astral.transform-first", false);
    assert_eq!(
        events::transform_outgoing_message("100", "create", "foo message").as_deref(),
        Some("foo message!")
    );
    stop_host();
    assert!(!events::has_outgoing_message_transforms());
}

fn composer_styles(work: &Path) {
    let root = work.join("composer-format-host");
    plugin(
        &root,
        "formatter",
        &manifest("astral.composer-test", &["ui.composer"]),
        "astral.ui.composer(function(text)\n  return {{start = 1, length = 2, style = 'bold'}, {start = 2, length = 1, style = 'italic'}, {start = 1, length = #text, style = 'spoiler'}, {start = 3, length = 1, style = 'underline'}, {start = 1, length = 1, style = 'unknown'}}\nend)\n",
    );
    plugin(
        &root,
        "restricted",
        &manifest("astral.composer-restricted", &["storage"]),
        "local ok = pcall(function() astral.ui.composer(function() return {} end) end)\nastral.storage.set('allowed', ok)\n",
    );
    start_host(&root, developer_settings(&root), None);
    let styles = events::composer_styles("éx".as_bytes());
    assert_eq!(
        styles,
        json!([{"start": 1, "length": 2, "style": "bold"}, {"start": 1, "length": 3, "style": "spoiler"}, {"start": 3, "length": 1, "style": "underline"}])
    );
    assert_eq!(events::composer_styles("x".repeat(COMPOSER_LIMIT).as_bytes()), json!([]));
    assert_eq!(eval("astral.composer-restricted", "return astral.storage.get('allowed')"), "false");
    lua_host::set_enabled("astral.composer-test", false);
    assert_eq!(events::composer_styles("éx".as_bytes()), json!([]));
    stop_host();
}

fn message_background(work: &Path) {
    let root = work.join("message-background-host");
    plugin(
        &root,
        "decorator",
        &manifest("astral.message-background-test", &["ui.message_background"]),
        "astral.ui.message_background(function(message)\n  if message.author_id == '100000000000000001' then return {color = 0x80445566, radius = 9.5} end\n  return {color = '#123456', radius = 33}\nend)\n",
    );
    plugin(
        &root,
        "restricted",
        &manifest("astral.message-background-restricted", &["storage"]),
        "local ok = pcall(function() astral.ui.message_background(function() return {} end) end)\nastral.storage.set('allowed', ok)\n",
    );
    start_host(&root, developer_settings(&root), None);
    let mut context = AstralMessageBackgroundContext {
        message_id: 200_000_000_000_000_011,
        user_id: 100_000_000_000_000_001,
        channel_id: 200_000_000_000_000_007,
    };
    let mut style = AstralMessageBackgroundStyle::default();
    assert_eq!(
        unsafe { nodes::astral_lua_message_background(&raw const context, &raw mut style) },
        1
    );
    assert_eq!((style.color, style.radius), (0x8044_5566, 9.5));
    context.user_id = 100_000_000_000_000_002;
    assert_eq!(
        unsafe { nodes::astral_lua_message_background(&raw const context, &raw mut style) },
        0
    );
    assert_eq!(
        eval("astral.message-background-restricted", "return astral.storage.get('allowed')"),
        "false"
    );
    lua_host::set_enabled("astral.message-background-test", false);
    context.user_id = 100_000_000_000_000_001;
    assert_eq!(
        unsafe { nodes::astral_lua_message_background(&raw const context, &raw mut style) },
        0
    );
    stop_host();
}

fn patch_target(identifier: &CStr, kind: &CStr, key: usize, message: u64, author: u64) -> AstralPatchTarget {
    AstralPatchTarget {
        identifier: identifier.as_ptr(),
        kind: kind.as_ptr(),
        key,
        message_id: message,
        author_id: author,
        channel_id: 0,
        parent: core::ptr::null(),
        node: 0,
        checked: -1,
        label: core::ptr::null(),
    }
}

fn patched(identifier: &CStr, kind: &CStr, author: u64) -> Option<AstralNodePatch> {
    let target = patch_target(identifier, kind, "demo.card:".len(), 0, author);
    let mut patch = AstralNodePatch::default();
    (unsafe { nodes::astral_lua_patch_node(&raw const target, &raw mut patch) } == 1).then_some(patch)
}

fn node_patch(work: &Path) {
    let root = work.join("node-patch-host");
    plugin(
        &root,
        "patcher",
        &manifest("astral.node-patch-test", &["ui.patch"]),
        "astral.ui.patch({ id = 'demo.card:', type = 'view' }, function(node)\n  if node.key == 'hide' then return { hidden = true } end\n  if node.author_id == '100000000000000001' then return { text_color = 0xFF00FF00 } end\n  return { background = '#5865F2', radius = 6, padding = 4 }\nend)\n",
    );
    start_host(&root, developer_settings(&root), None);
    let card = patched(c"demo.card:42", c"view", 0).expect("card");
    assert_eq!((card.background, card.radius, card.padding[3]), (0xFF58_65F2, 6.0, 4.0));
    assert_eq!(card.fields, PATCH_BACKGROUND | PATCH_RADIUS | PATCH_PADDING);
    assert_eq!(patched(c"demo.card:hide", c"view", 0).expect("hidden").fields, PATCH_HIDDEN);
    assert_eq!(
        patched(c"demo.card:7", c"view", 100_000_000_000_000_001)
            .expect("authored")
            .text_color,
        0xFF00_FF00
    );
    assert!(patched(c"demo.card:42", c"text", 0).is_none());
    assert!(patched(c"other:42", c"view", 0).is_none());
    stop_host();
}

fn node_insert(work: &Path) {
    let root = work.join("node-insert-host");
    plugin(
        &root,
        "inserter",
        &manifest("astral.node-insert-test", &["ui.insert"]),
        "astral.ui.insert('message.content:', function(node)\n  return { type = 'view', row = true, background = '#202020', action = 'open', position = 'start',\n    children = { { type = 'text', text = 'Hi ' .. node.key, color = 0xFFFFFFFF, bold = true } } }\nend, function(node, action) astral.state.pressed = action .. ':' .. node.message_id end)\n",
    );
    start_host(&root, developer_settings(&root), None);
    let target = patch_target(
        c"message.content:200000000000000011",
        c"view",
        "message.content:".len(),
        200_000_000_000_000_011,
        0,
    );
    let mut inserted = vec![AstralInsertNode::empty(); INSERT_CAPACITY];
    let mut at_start = 0;
    let count = unsafe { nodes::astral_lua_insert_nodes(&raw const target, inserted.as_mut_ptr(), INSERT_CAPACITY, &raw mut at_start) };
    assert_eq!((count, at_start), (2, 1));
    assert!(inserted[0].kind == INSERT_VIEW && inserted[0].parent == -1 && inserted[0].fields & INSERT_ROW != 0);
    assert_eq!(inserted[0].background, 0xFF20_2020);
    assert!(inserted[1].kind == INSERT_TEXT && inserted[1].parent == 0);
    assert_eq!(c_text(&inserted[1].text), "Hi 200000000000000011");
    let tap = c_text(&inserted[0].tap);
    assert!(tap.starts_with("astral.tap.") && tap.contains(":open:200000000000000011"));
    assert!(nodes::press_inserted_node(&tap));
    assert_eq!(
        eval("astral.node-insert-test", "return astral.state.pressed"),
        "open:200000000000000011"
    );
    assert!(!nodes::press_inserted_node("astral.tap.999999:open:1"));
    stop_host();
}

fn compiled_chunk(source: &str, chunk: &str) -> Vec<u8> {
    use astral_core::lua::api as lua;
    unsafe extern "C" fn collect(_: *mut lua::LuaState, bytes: *const core::ffi::c_void, size: usize, out: *mut core::ffi::c_void) -> i32 {
        let out = unsafe { &mut *out.cast::<Vec<u8>>() };
        out.extend_from_slice(unsafe { core::slice::from_raw_parts(bytes.cast::<u8>(), size) });
        0
    }
    let state = unsafe { lua::luaL_newstate() };
    let name = std::ffi::CString::new(chunk).expect("chunk");
    assert_eq!(
        unsafe { lua::luaL_loadbufferx(state, source.as_ptr().cast(), source.len(), name.as_ptr(), c"t".as_ptr()) },
        0
    );
    let mut bytecode = Vec::new();
    unsafe { lua::lua_dump(state, collect, (&raw mut bytecode).cast(), 0) };
    unsafe { lua::lua_close(state) };
    bytecode
}

fn bytecode_only_when_signed(work: &Path) {
    let root = work.join("bytecode-host");
    for name in ["signed", "unsigned"] {
        plugin(
            &root,
            name,
            &format!("id = 'astral.bytecode-{name}'\nname = 'Bytecode'\nversion = '1.0'\nmain = 'main.lua'\n"),
            "astral.state.from = 'source'\nastral.state.module = require('lib.helper')\n",
        );
        write(&root.join("plugins").join(name).join("lib/helper.lua"), "return 'source module'\n");
    }
    std::fs::write(
        root.join("plugins/signed/main.luac"),
        compiled_chunk(
            "astral.state.from = 'bytecode'\nastral.state.module = require('lib.helper')\n",
            "=astral.bytecode-signed",
        ),
    )
    .expect("main");
    std::fs::write(
        root.join("plugins/signed/lib/helper.luac"),
        compiled_chunk("return 'bytecode module'\n", "=astral.bytecode-signed/lib.helper"),
    )
    .expect("helper");
    std::fs::write(root.join("plugins/unsigned/main.luac"), b"\x1bLua not really bytecode").expect("fake");
    on("verifySignature", |arguments| {
        json!(!arguments[0].as_str().unwrap_or_default().contains("unsigned"))
    });
    start_host(&root, developer_settings(&root), None);
    assert_eq!(
        eval("astral.bytecode-signed", "return astral.state.from .. ':' .. astral.state.module"),
        "bytecode:bytecode module"
    );
    assert_eq!(
        eval("astral.bytecode-unsigned", "return astral.state.from .. ':' .. astral.state.module"),
        "source:source module"
    );
    stop_host();
}

#[derive(Clone, Default)]
struct NativeUi {
    alert: (String, String),
    toast: (String, String),
    clipboard: String,
    authorized: u64,
    authorization: (String, String),
    picked: u64,
    extensions: Vec<String>,
    cancelled_picks: Vec<u64>,
    cancelled_auth: Vec<u64>,
    opened_sheet: u64,
    closed_sheet: u64,
    sheet: Value,
    updated_sheet: Value,
    confirm: u64,
    confirm_spec: Value,
}

fn text_at(arguments: &Value, index: usize) -> String {
    arguments[index].as_str().unwrap_or_default().to_owned()
}

fn id_at(arguments: &Value, index: usize) -> u64 {
    arguments[index].as_u64().unwrap_or(0)
}

fn native_ui_platform(state: &Arc<Mutex<NativeUi>>) {
    let with = |state: &Arc<Mutex<NativeUi>>, work: fn(&mut NativeUi, &Value) -> Value| {
        let state = state.clone();
        move |arguments: &Value| work(&mut state.lock().expect("ui"), arguments)
    };
    on(
        "showAlert",
        with(state, |ui, arguments| {
            ui.alert = (text_at(arguments, 0), text_at(arguments, 1));
            Value::Null
        }),
    );
    on(
        "showToast",
        with(state, |ui, arguments| {
            ui.toast = (text_at(arguments, 0), text_at(arguments, 1));
            Value::Null
        }),
    );
    on(
        "webAuth",
        with(state, |ui, arguments| {
            if ui.authorized == 0 {
                ui.authorized = id_at(arguments, 0);
            }
            ui.authorization = (text_at(arguments, 1), text_at(arguments, 2));
            json!(true)
        }),
    );
    on(
        "cancelWebAuth",
        with(state, |ui, arguments| {
            ui.cancelled_auth.push(id_at(arguments, 0));
            Value::Null
        }),
    );
    on(
        "openSheet",
        with(state, |ui, arguments| {
            ui.opened_sheet = id_at(arguments, 0);
            ui.sheet = arguments[1].clone();
            json!(true)
        }),
    );
    on(
        "updateSheet",
        with(state, |ui, arguments| {
            ui.updated_sheet = arguments[1].clone();
            Value::Null
        }),
    );
    on(
        "closeSheet",
        with(state, |ui, arguments| {
            ui.closed_sheet = id_at(arguments, 0);
            Value::Null
        }),
    );
    on(
        "confirm",
        with(state, |ui, arguments| {
            ui.confirm = id_at(arguments, 0);
            ui.confirm_spec = arguments[1].clone();
            json!(true)
        }),
    );
    on(
        "writeClipboard",
        with(state, |ui, arguments| {
            ui.clipboard = text_at(arguments, 0);
            json!(true)
        }),
    );
    on(
        "pickFile",
        with(state, |ui, arguments| {
            ui.picked = id_at(arguments, 0);
            ui.extensions = arguments[1]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect();
            json!(true)
        }),
    );
    on(
        "cancelFilePick",
        with(state, |ui, arguments| {
            ui.cancelled_picks.push(id_at(arguments, 0));
            Value::Null
        }),
    );
}

fn file_picker_and_dialogs(work: &Path) {
    let root = work.join("native-ui-host");
    plugin(
        &root,
        "native-ui",
        &manifest(
            "astral.native-ui-test",
            &[
                "files.read",
                "ui.dialogs",
                "ui.clipboard",
                "ui.toasts",
                "ui.web_auth",
                "events.lifecycle",
                "storage",
            ],
        ),
        "astral.ui.alert('Ready', 'Choose a file')\nastral.ui.toast('Status', 'Ready')\nastral.ui.copy('exported rules')\nlocal auth = astral.ui.web_auth('https://example.com/auth', 'https://example.com/callback', function(url, cookies, error)\n  astral.storage.set('authorization', {url = url, cookies = cookies, error = error})\nend)\nastral.storage.set('auth_request', auth)\nlocal request = astral.files.pick({'json', '.TOML', 'json'}, function(file, reason)\n  if file then astral.storage.set('file', {name = file.name, data = file.data})\n  else astral.storage.set('failure', reason) end\nend)\nastral.storage.set('request', request)\nlocal subscription = astral.events.on('app.foreground', function(event) astral.storage.set('event', event) end)\nastral.storage.set('subscription', subscription)\n",
    );
    plugin(
        &root,
        "restricted",
        &manifest("astral.native-ui-restricted", &["storage"]),
        "local alertOK = pcall(function() astral.ui.alert('No', 'permission') end)\nlocal toastOK = pcall(function() astral.ui.toast('No', 'permission') end)\nlocal authOK = pcall(function() astral.ui.web_auth('https://example.com/auth', 'https://example.com/callback', function() end) end)\nlocal pickerOK = pcall(function() astral.files.pick({'json'}, function() end) end)\nlocal copyOK = pcall(function() astral.ui.copy('private') end)\nlocal eventOK = pcall(function() astral.events.on('app.foreground', function() end) end)\nastral.storage.set('permissions', {alert = alertOK, toast = toastOK, auth = authOK, picker = pickerOK, copy = copyOK, event = eventOK})\n",
    );
    let state = Arc::new(Mutex::new(NativeUi::default()));
    native_ui_platform(&state);
    let ui = || state.lock().expect("ui").clone();
    start_host(&root, developer_settings(&root), None);
    assert_eq!(ui().alert, ("Ready".to_owned(), "Choose a file".to_owned()));
    assert_eq!(
        eval(
            "astral.native-ui-test",
            "return tostring(astral.ui.confirm({ title = 'Send?', image = 'https://cdn.example/s.png', confirm = 'Send' }, function(ok) astral.storage.set('confirmed', ok) end) > 0)"
        ),
        "true"
    );
    let spec = ui().confirm_spec;
    assert_eq!(
        (spec["title"].as_str(), spec["confirm"].as_str(), spec["cancel"].as_str()),
        (Some("Send?"), Some("Send"), Some("Cancel"))
    );
    assert_eq!(spec["image"], "https://cdn.example/s.png");
    lua_host::complete_confirm(ui().confirm, true);
    assert_eq!(
        eval(
            "astral.native-ui-test",
            "sheet = astral.ui.sheet({ title = 'Raw', items = { { type = 'code', text = '{}' } } }, function(action, value) astral.storage.set('sheet_event', action .. ':' .. value) end); return tostring(sheet.id > 0)"
        ),
        "true"
    );
    assert_eq!(
        (ui().sheet["title"].as_str(), ui().sheet["items"][0]["type"].as_str()),
        (Some("Raw"), Some("code"))
    );
    lua_host::sheet_event(ui().opened_sheet, "copy", "data", false);
    assert_eq!(
        eval("astral.native-ui-test", "return astral.storage.get('sheet_event')"),
        "copy:data"
    );
    eval("astral.native-ui-test", "sheet:update({ title = 'Raw 2' }); sheet:close()");
    assert_eq!(ui().updated_sheet["title"], "Raw 2");
    assert_eq!(ui().closed_sheet, ui().opened_sheet);
    lua_host::sheet_event(ui().opened_sheet, "", "", true);
    eval(
        "astral.native-ui-test",
        "astral.settings.section('Account'):button('authorize', 'Authorize', function() astral.storage.set('pressed', 'yes') end, { subtitle = 'Sign in' })",
    );
    let listed = lua_host::plugins_json().as_array().into_iter().flatten().any(|plugin| {
        plugin["settingsButtons"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|button| button["key"] == "authorize" && button["section"] == "Account")
    });
    assert!(listed);
    ui::press_settings_button("astral.native-ui-test", "authorize");
    assert_eq!(eval("astral.native-ui-test", "return astral.storage.get('pressed')"), "yes");
    assert_eq!(eval("astral.native-ui-test", "return astral.storage.get('sheet_event')"), "closed:");
    assert_eq!(
        eval("astral.native-ui-test", "return tostring(astral.storage.get('confirmed'))"),
        "true"
    );
    assert_eq!(
        eval(
            "astral.native-ui-test",
            "return tostring(pcall(astral.ui.confirm, { title = 'x', image = 'http://a' }, function() end))"
        ),
        "false"
    );
    assert_eq!(ui().toast, ("Status".to_owned(), "Ready".to_owned()));
    assert_eq!(ui().clipboard, "exported rules");
    let authorized = ui().authorized;
    assert!(authorized > 0);
    assert_eq!(
        ui().authorization,
        ("https://example.com/auth".to_owned(), "https://example.com/callback".to_owned())
    );
    assert_eq!(
        number(&eval("astral.native-ui-test", "return astral.storage.get('auth_request')")),
        authorized
    );
    assert_eq!(
        eval(
            "astral.native-ui-test",
            "local ok = pcall(function() astral.ui.web_auth('https://example.com/auth', 'https://example.com/callback', function() end) end); return ok"
        ),
        "false"
    );
    assert!(ui().picked > 0);
    assert_eq!(ui().extensions, vec!["json".to_owned(), "toml".to_owned()]);
    assert_eq!(
        number(&eval("astral.native-ui-test", "return astral.storage.get('request')")),
        ui().picked
    );
    lua_host::complete_file_pick(ui().picked, Some(b"theme.json"), Some(br#"{"name":"fixture"}"#), None);
    assert_eq!(
        eval("astral.native-ui-test", "return astral.storage.get('file').name"),
        "theme.json"
    );
    assert_eq!(
        eval("astral.native-ui-test", "return astral.storage.get('file').data"),
        r#"{"name":"fixture"}"#
    );
    assert_eq!(
        eval(
            "astral.native-ui-restricted",
            "local p = astral.storage.get('permissions'); return tostring(p.alert) .. ':' .. tostring(p.toast) .. ':' .. tostring(p.auth) .. ':' .. tostring(p.picker) .. ':' .. tostring(p.copy) .. ':' .. tostring(p.event)"
        ),
        "false:false:false:false:false:false"
    );
    assert_eq!(
        eval(
            "astral.native-ui-test",
            "local ok = pcall(function() astral.ui.web_auth('https://example.com/auth', 'https://other.example/callback', function() end) end); return ok"
        ),
        "false"
    );
    lua_host::complete_web_auth(
        authorized,
        Some("https://example.com/callback?code=ok".to_owned()),
        Some("session=ok".to_owned()),
        None,
    );
    assert_eq!(
        eval("astral.native-ui-test", "return astral.storage.get('authorization').url"),
        "https://example.com/callback?code=ok"
    );
    assert_eq!(
        eval("astral.native-ui-test", "return astral.storage.get('authorization').cookies"),
        "session=ok"
    );
    assert_eq!(
        eval(
            "astral.native-ui-test",
            "local request = astral.ui.web_auth('https://discord.com/oauth2/authorize?client_id=1&redirect_uri=https%3A%2F%2Fother.example%2Fcallback%2Fauth', 'https://other.example/callback', function() end); astral.ui.cancel_web_auth(request); return tostring(request > 0)"
        ),
        "true"
    );
    assert_eq!(
        eval(
            "astral.native-ui-test",
            "local ok = pcall(function() astral.ui.web_auth('https://discord.com/oauth2/authorize?redirect_uri=https%3A%2F%2Fother.example%2Fcallback', 'https://third.example/callback', function() end) end); return ok"
        ),
        "false"
    );
    let cancelled_auth = number(&eval(
        "astral.native-ui-test",
        "local request = astral.ui.web_auth('https://example.com/auth', 'https://example.com/callback', function() astral.storage.set('cancelled_auth_callback', true) end); astral.ui.cancel_web_auth(request); return request",
    ));
    assert_ne!(cancelled_auth, authorized);
    assert_eq!(ui().cancelled_auth.last(), Some(&cancelled_auth));
    lua_host::complete_web_auth(cancelled_auth, Some("https://example.com/callback".to_owned()), None, None);
    assert_eq!(
        eval(
            "astral.native-ui-test",
            "return astral.storage.get('cancelled_auth_callback') == nil"
        ),
        "true"
    );
    let subscription = number(&eval("astral.native-ui-test", "return astral.storage.get('subscription')"));
    lua_host::emit_event("app.foreground");
    assert_eq!(
        eval("astral.native-ui-test", "return astral.storage.get('event')"),
        "app.foreground"
    );
    assert_eq!(
        eval(
            "astral.native-ui-test",
            "return astral.events.off(tonumber(astral.storage.get('subscription')))"
        ),
        "true"
    );
    eval("astral.native-ui-test", "astral.storage.remove('event')");
    lua_host::emit_event("app.foreground");
    assert_eq!(eval("astral.native-ui-test", "return astral.storage.get('event') == nil"), "true");
    assert_eq!(
        eval("astral.native-ui-test", &format!("return astral.events.off({subscription})")),
        "false"
    );
    let cancelled_pick = number(&eval(
        "astral.native-ui-test",
        "local request = astral.files.pick({'json'}, function(_, reason) astral.storage.set('cancelled_callback', reason) end); astral.storage.set('cancelled_request', request); return request",
    ));
    assert_eq!(
        eval(
            "astral.native-ui-test",
            "return astral.files.cancel(tonumber(astral.storage.get('cancelled_request')))"
        ),
        "true"
    );
    assert_eq!(ui().cancelled_picks.last(), Some(&cancelled_pick));
    lua_host::complete_file_pick(cancelled_pick, None, None, Some("cancelled"));
    assert_eq!(
        eval("astral.native-ui-test", "return astral.storage.get('cancelled_callback') == nil"),
        "true"
    );
    let oversized = number(&eval(
        "astral.native-ui-test",
        "local request = astral.files.pick({'json'}, function(file, reason) astral.storage.set('oversized_callback', reason) end); astral.storage.set('oversized_request', request); return request",
    ));
    lua_host::complete_file_pick(oversized, Some(b"large.json"), Some(&vec![b'x'; FILE_LIMIT + 1]), None);
    assert_eq!(
        eval("astral.native-ui-test", "return astral.storage.get('oversized_callback')"),
        "file exceeds 1 MiB"
    );
    let stopped = number(&eval(
        "astral.native-ui-test",
        "local request = astral.files.pick({'json'}, function() end); astral.storage.set('stopped_request', request); return request",
    ));
    stop_host();
    assert_eq!(state.lock().expect("ui").cancelled_picks.last(), Some(&stopped));
}

fn padded(identifier: &str) -> String {
    format!("{identifier:0>20}")
}

fn discord_store(directory: &Path, account: &str, rows: &[(&str, String, Value)]) {
    let folder = directory.join("store").join(account);
    std::fs::create_dir_all(&folder).expect("store");
    use astral_core::support::sqlite_api as sqlite;
    let path = folder.join("cache.sqlite").to_string_lossy().into_owned();
    let database = sqlite::Database::open(&path, sqlite::OPEN_READWRITE | sqlite::OPEN_CREATE).expect("database");
    for table in ["messages", "members", "channels", "guilds", "users"] {
        assert!(database.execute(&format!("CREATE TABLE {table} (k TEXT PRIMARY KEY, v BLOB)")));
    }
    for (table, key, value) in rows {
        let text = value.to_string();
        let mut statement = database.prepare(
            &format!("INSERT INTO {table} (k, v) VALUES (?, ?)"),
            &[key.as_bytes(), text.as_bytes()],
        );
        assert_eq!(statement.step(), sqlite::DONE);
    }
}

fn process(message: &str) -> Option<Value> {
    events::process_gateway_message(message.as_bytes()).map(|text| serde_json::from_str(&text).expect("json"))
}

fn gateway_message_events_and_haptics(work: &Path) {
    let root = work.join("gateway-events-host");
    plugin(
        &root,
        "mention-haptics",
        &manifest(
            "astral.gateway-events-test",
            &[
                "discord.messages",
                "events.messages",
                "events.messages.filter",
                "ui.haptics",
                "ui.message_actions",
                "storage",
            ],
        ),
        "astral.events.on('message.filter', function(message) return message.content:find('filtered', 1, true) ~= nil end)\nastral.ui.message_color(function(message) if message.reply_to_me then return 0x805865F2 end end)\nastral.ui.message_action('Inspect', function(message) astral.storage.set('action', message.message_id .. ':' .. message.channel_id) end)\nastral.events.on('message.create', function(message)\n  astral.storage.set('event', message)\n  if not message.from_me and not (message.open and not astral.settings.get('open', false)) and (message.mentioned or message.direct) then astral.ui.haptic('medium') end\nend)\n",
    );
    plugin(
        &root,
        "restricted",
        &manifest("astral.gateway-events-restricted", &["storage"]),
        "local events = pcall(function() astral.events.on('message.create', function() end) end)\nlocal filters = pcall(function() astral.events.on('message.filter', function() end) end)\nlocal haptics = pcall(function() astral.ui.haptic('light') end)\nlocal actions = pcall(function() astral.ui.message_action('Inspect', function() end) end)\nastral.ui.message_color(function(message) astral.storage.set('reply_context', tostring(message.reply_to_me)) end)\nastral.storage.set('permissions', {events = events, filters = filters, haptics = haptics, actions = actions})\n",
    );
    plugin(
        &root,
        "content-reader",
        &manifest(
            "astral.gateway-content-reader",
            &["discord.messages", "discord.messages.content", "storage"],
        ),
        "local message = astral.discord.message('200000000000000007', '200000000000000010')\nastral.storage.set('content', message.content)\n",
    );
    let direct = padded("200000000000000007");
    let reference = json!({"channel_id": "200000000000000007", "message_id": "200000000000000010"});
    discord_store(
        &root.join("discord"),
        "100000000000000001",
        &[
            (
                "messages",
                format!("{direct}:{}", padded("200000000000000010")),
                json!({"content": "cached message text", "author": {"id": "100000000000000001"}}),
            ),
            (
                "messages",
                format!("{direct}:{}", padded("200000000000000011")),
                json!({"author": {"id": "200000000000000003"}, "message_reference": reference}),
            ),
            (
                "messages",
                format!("{direct}:{}", padded("200000000000000012")),
                json!({"author": {"id": "100000000000000001"}, "message_reference": reference}),
            ),
            (
                "members",
                format!("{}:{}", padded("200000000000000005"), padded("100000000000000001")),
                json!({"roles": ["300000000000000001"]}),
            ),
        ],
    );
    let strengths = recorder::<String>();
    let record = strengths.clone();
    on("triggerHaptic", move |arguments| {
        push(&record, text_at(arguments, 0));
        json!(true)
    });
    start_host(&root, developer_settings(&root), Some(&root.join("discord")));
    let medium = |count: usize| vec!["medium".to_owned(); count];
    assert_eq!(
        eval(
            "astral.gateway-events-test",
            "return astral.discord.message('200000000000000007', '200000000000000010').content == nil"
        ),
        "true"
    );
    assert_eq!(
        eval("astral.gateway-content-reader", "return astral.storage.get('content')"),
        "cached message text"
    );
    assert_eq!(
        eval(
            "astral.gateway-events-restricted",
            "local p = astral.storage.get('permissions'); return tostring(p.events) .. ':' .. tostring(p.filters) .. ':' .. tostring(p.haptics) .. ':' .. tostring(p.actions)"
        ),
        "false:false:false:false"
    );
    assert!(process(r#"{"op":0,"t":"READY","d":{"user":{"id":"100000000000000001"}}}"#).is_none());
    assert!(process(r#"{"op":0,"t":"MESSAGE_CREATE","d":{"id":"200000000000000001","channel_id":"200000000000000002","author":{"id":"200000000000000003"},"mentions":[{"id":"100000000000000001"}],"content":"private message text"}}"#).is_none());
    assert_eq!(recorded(&strengths), medium(1));
    assert_eq!(
        eval("astral.gateway-events-test", "return astral.storage.get('event').mentioned"),
        "true"
    );
    assert_eq!(
        eval("astral.gateway-events-test", "return astral.storage.get('event').content == nil"),
        "true"
    );
    assert!(process(r#"{"op":0,"t":"MESSAGE_CREATE","d":{"id":"200000000000000004","channel_id":"200000000000000002","guild_id":"200000000000000005","author":{"id":"200000000000000003"},"mentions":[],"content":"private message text"}}"#).is_none());
    assert_eq!(recorded(&strengths), medium(1));
    assert!(process(r#"{"op":0,"t":"MESSAGE_CREATE","d":{"id":"200000000000000006","channel_id":"200000000000000007","author":{"id":"200000000000000003"},"mentions":[],"content":"private direct message"}}"#).is_none());
    assert_eq!(recorded(&strengths), medium(2));
    let skipped = process(r#"{"op":0,"s":10,"t":"MESSAGE_CREATE","d":{"id":"200000000000000008","channel_id":"200000000000000007","author":{"id":"200000000000000003"},"mentions":[],"content":"filtered text"}}"#).expect("filtered");
    assert_eq!(
        (skipped["op"].as_i64(), skipped["s"].as_i64(), skipped["t"].as_str()),
        (Some(0), Some(10), Some("ASTRAL_FILTERED"))
    );
    let deletion = process(r#"{"op":0,"s":11,"t":"MESSAGE_UPDATE","d":{"id":"200000000000000015","channel_id":"200000000000000002","guild_id":"200000000000000005","author":{"id":"200000000000000003"},"content":"now filtered"}}"#).expect("removed");
    assert_eq!(
        (deletion["t"].as_str(), deletion["s"].as_i64(), deletion["d"]["id"].as_str()),
        (Some("MESSAGE_DELETE"), Some(11), Some("200000000000000015"))
    );
    assert!(deletion["d"]["guild_id"] == "200000000000000005" && deletion["d"].get("content").is_none());
    assert!(process(r#"{"op":0,"t":"MESSAGE_UPDATE","d":{"id":"200000000000000016","channel_id":"200000000000000002","author":{"id":"200000000000000003"},"content":"harmless edit"}}"#).is_none());
    assert_eq!(
        eval("astral.gateway-events-test", "return astral.storage.get('event').message_id"),
        "200000000000000006"
    );
    assert_eq!(recorded(&strengths), medium(2));
    assert!(process(r#"{"op":0,"t":"MESSAGE_CREATE","d":{"id":"200000000000000009","channel_id":"200000000000000007","author":{"id":"100000000000000001"},"mentions":[{"id":"100000000000000001"}],"content":"filtered own message"}}"#).is_none());
    assert_eq!(recorded(&strengths), medium(2));
    assert!(process(r#"{"op":0,"t":"MESSAGE_CREATE","d":{"id":"200000000000000013","channel_id":"200000000000000002","guild_id":"200000000000000005","author":{"id":"200000000000000003"},"mentions":[],"mention_roles":["300000000000000001"],"content":"role ping"}}"#).is_none());
    assert_eq!(
        eval(
            "astral.gateway-events-test",
            "local e = astral.storage.get('event'); return tostring(e.role_mentioned) .. ':' .. tostring(e.everyone) .. ':' .. e.guild_id"
        ),
        "true:false:200000000000000005"
    );
    assert!(process(r#"{"op":0,"t":"MESSAGE_CREATE","d":{"id":"200000000000000014","channel_id":"200000000000000002","guild_id":"200000000000000005","author":{"id":"200000000000000003"},"mentions":[],"mention_roles":["300000000000000002"],"mention_everyone":true,"content":"everyone ping"}}"#).is_none());
    assert_eq!(
        eval(
            "astral.gateway-events-test",
            "local e = astral.storage.get('event'); return tostring(e.role_mentioned) .. ':' .. tostring(e.everyone)"
        ),
        "false:true"
    );
    let mut color = 0u32;
    assert_eq!(
        unsafe {
            astral_core::lua::host::hooks::astral_lua_message_text_color(200_000_000_000_000_011, 200_000_000_000_000_007, &raw mut color)
        },
        1
    );
    assert_eq!(color, 0x8058_65F2);
    assert_eq!(
        eval("astral.gateway-events-restricted", "return astral.storage.get('reply_context')"),
        "nil"
    );
    assert_eq!(
        unsafe {
            astral_core::lua::host::hooks::astral_lua_message_text_color(200_000_000_000_000_012, 200_000_000_000_000_007, &raw mut color)
        },
        0
    );
    let context = AstralDecorationContext {
        prefix: c"message.author:".as_ptr(),
        message_id: 200_000_000_000_000_011,
        user_id: 200_000_000_000_000_003,
        channel_id: 200_000_000_000_000_007,
    };
    let mut tags = [AstralTag::zeroed(); 4];
    assert_eq!(
        unsafe { nodes::astral_lua_decorate(&raw const context, tags.as_mut_ptr(), tags.len()) },
        0
    );
    let mut entries: [AstralMenuEntry; 4] = unsafe { core::mem::zeroed() };
    assert_eq!(unsafe { nodes::astral_lua_message_menu(entries.as_mut_ptr(), entries.len()) }, 1);
    assert_eq!(c_text(&entries[0].label), "Inspect");
    let action = c_text(&entries[0].action_id);
    assert!(action.starts_with("astral.action."));
    assert!(!nodes::press_message_action(&action));
    nodes::set_menu_message(200_000_000_000_000_007, 200_000_000_000_000_011);
    assert!(nodes::press_message_action(&action));
    assert_eq!(
        eval("astral.gateway-events-test", "return astral.storage.get('action')"),
        "200000000000000011:200000000000000007"
    );
    stop_host();
}

fn request(method: &str, url: &str, path: &str, headers: &[(&str, &str)], body: &str) -> HttpRequest {
    HttpRequest {
        method: method.to_owned(),
        url: url.to_owned(),
        path: path.to_owned(),
        headers: headers
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect(),
        body: body.as_bytes().to_vec(),
    }
}

fn gateway_and_http_intercepts(work: &Path) {
    let root = work.join("intercept-host");
    plugin(
        &root,
        "intercept",
        &manifest("astral.intercept", &["events.gateway", "messages.local", "net.discord"]),
        "seen = {}\nastral.events.on('gateway', function(event)\n  seen[#seen + 1] = event.type\n  if event.type == 'MESSAGE_DELETE' then return false end\n  if event.type == 'TYPING_START' then event.data.tagged = true return event.data end\nend, {'MESSAGE_DELETE', 'TYPING_START'})\nastral.net.intercept({method = 'GET', path = '^/api/v%d+/gifs/search', request = function(request)\n  auth = request.headers.authorization\n  return {url = request.url .. '&provider=tenor', headers = {['x-astral'] = '1'}}\nend, response = function(request, response)\n  local body = astral.json.decode(response.body)\n  body[#body + 1] = {id = 'extra'}\n  return {body = astral.json.encode(body)}\nend})\nastral.net.intercept({method = 'POST', path = '/stickers$', request = function(request)\n  held = request.id\n  return astral.net.HOLD\nend})\nastral.net.intercept({method = 'POST', path = '/interactions$', request = function()\n  return {status = 204}\nend})\ndispatched = astral.gateway.dispatch('MESSAGE_CREATE', {id = '1', mentions = astral.json.array()})\nlocalID = astral.messages.send_local('100000000000000002', {content = 'hi', embeds = {{title = 'x'}}})\nroundtrip = astral.json.encode(astral.json.decode('{\"a\":[],\"b\":{}}'))\n",
    );
    let dispatched = recorder::<String>();
    let record = dispatched.clone();
    on("dispatchGateway", move |arguments| {
        push(&record, text_at(arguments, 0));
        json!(true)
    });
    start_host(&root, developer_settings(&root), None);
    assert!(events::needs_gateway_events());
    assert_eq!(eval("astral.intercept", "return dispatched"), "true");
    let messages = recorded(&dispatched);
    let first: Value = serde_json::from_str(&messages[0]).expect("dispatched");
    assert!(first["t"] == "MESSAGE_CREATE" && first["d"]["mentions"].is_array());
    assert_eq!(eval("astral.intercept", "return roundtrip"), r#"{"a":[],"b":{}}"#);
    let local: Value = serde_json::from_str(messages.last().expect("local")).expect("local");
    assert!(local["d"]["content"] == "hi" && local["d"]["flags"] == 64 && local["d"]["mentions"].is_array());
    assert_eq!(local["d"]["embeds"][0]["title"], "x");
    assert_eq!(local["d"]["timestamp"].as_str().map(str::len), Some(32));
    assert_eq!(
        eval("astral.intercept", "return localID"),
        local["d"]["id"].as_str().unwrap_or_default()
    );
    assert_eq!(
        process(r#"{"op":0,"t":"MESSAGE_DELETE","d":{"id":"5"}}"#).expect("dropped")["t"],
        "ASTRAL_FILTERED"
    );
    assert_eq!(
        process(r#"{"op":0,"t":"TYPING_START","s":4,"d":{"user_id":"7"}}"#).expect("tagged")["d"]["tagged"],
        true
    );
    assert!(process(r#"{"op":0,"t":"PRESENCE_UPDATE","d":{}}"#).is_none());
    assert_eq!(eval("astral.intercept", "return #seen"), "2");
    assert!(http::wants_http("GET", "discord.com", "/api/v9/gifs/search"));
    assert!(!http::wants_http("GET", "example.com", "/api/v9/gifs/search"));
    assert!(!http::wants_http("GET", "discord.com", "/api/v9/users/@me"));
    let mut search = request(
        "GET",
        "https://discord.com/api/v9/gifs/search?q=cat",
        "/api/v9/gifs/search",
        &[("authorization", "secret"), ("accept", "application/json")],
        "",
    );
    let decision = http::intercept_request(&mut search);
    assert!(decision.changed && decision.response.is_none());
    assert_eq!(search.url, "https://discord.com/api/v9/gifs/search?q=cat&provider=tenor");
    assert_eq!(eval("astral.intercept", "return tostring(auth)"), "nil");
    assert!(
        search
            .headers
            .iter()
            .any(|(name, value)| name == "authorization" && value == "secret")
    );
    assert!(search.headers.iter().any(|(name, _)| name == "x-astral"));
    let mut results = HttpResponse {
        status: 200,
        headers: vec![("content-type".to_owned(), "application/json".to_owned())],
        body: br#"[{"id":"a"}]"#.to_vec(),
    };
    assert!(http::intercept_response(&search, &mut results));
    assert_eq!(
        serde_json::from_slice::<Value>(&results.body)
            .expect("body")
            .as_array()
            .map(Vec::len),
        Some(2)
    );
    let mut interaction = request("POST", "https://discord.com/api/v9/interactions", "/api/v9/interactions", &[], "{}");
    assert_eq!(
        http::intercept_request(&mut interaction).response.map(|response| response.status),
        Some(204)
    );
    let mut sticker = request(
        "POST",
        "https://discord.com/api/v9/channels/1/stickers",
        "/api/v9/channels/1/stickers",
        &[],
        "{}",
    );
    let releaser = std::thread::spawn(|| {
        for _ in 0..HOLD_ATTEMPTS {
            std::thread::sleep(HOLD_POLL);
            if eval("astral.intercept", "return tostring(held)") != "nil" {
                break;
            }
        }
        eval("astral.intercept", "astral.net.release(held, {body = 'released'})");
    });
    let held = http::intercept_request(&mut sticker);
    releaser.join().expect("releaser");
    assert!(held.changed && sticker.body == b"released");
    stop_host();
    assert!(!events::needs_gateway_events());
}

fn commands(work: &Path) {
    let root = work.join("commands-host");
    plugin(
        &root,
        "commands",
        &manifest("astral.commands-test", &["commands"]),
        "astral.commands.register({name = 'echo', description = 'Echo', options = {{name = 'text', type = 'string', required = true}},\n  execute = function(args, context) last = context.channel_id return 'you said ' .. args.text end})\nastral.commands.register({name = 'post', description = 'Post', execute = function()\n  return {send = {content = 'card', file = {name = 'card.png', data = 'PNGDATA'}}} end})\n",
    );
    let dispatched = recorder::<String>();
    let record = dispatched.clone();
    on("dispatchGateway", move |arguments| {
        push(&record, text_at(arguments, 0));
        json!(true)
    });
    start_host(&root, developer_settings(&root), None);
    assert!(http::wants_http(
        "GET",
        "discord.com",
        "/api/v9/channels/1/application-command-index"
    ));
    let index = request(
        "GET",
        "https://discord.com/api/v9/channels/1/application-command-index",
        "/api/v9/channels/1/application-command-index",
        &[],
        "",
    );
    let mut indexed = HttpResponse {
        status: 200,
        headers: Vec::new(),
        body: br#"{"applications":[],"application_commands":[],"version":"1"}"#.to_vec(),
    };
    assert!(http::intercept_response(&index, &mut indexed));
    let listed: Value = serde_json::from_slice(&indexed.body).expect("commands");
    assert_eq!(listed["applications"][0]["name"], "astral");
    assert_eq!(listed["application_commands"].as_array().map(Vec::len), Some(2));
    assert_eq!(listed["application_commands"][0]["options"][0]["type"], 3);
    let echo = json!({"type": 2, "application_id": "1000000000000000001", "channel_id": "100000000000000002", "data": {"name": "echo", "options": [{"name": "text", "type": 3, "value": "hi"}]}});
    let multipart = format!("--x\r\nContent-Disposition: form-data; name=\"payload_json\"\r\n\r\n{echo}\r\n--x--\r\n");
    let mut interaction = request(
        "POST",
        "https://discord.com/api/v9/interactions",
        "/api/v9/interactions",
        &[("content-type", "multipart/form-data; boundary=x"), ("authorization", "secret")],
        &multipart,
    );
    assert_eq!(
        http::intercept_request(&mut interaction).response.map(|response| response.status),
        Some(204)
    );
    let local: Value = serde_json::from_str(&last(&dispatched).expect("local")).expect("local");
    assert_eq!(local["d"]["content"], "you said hi");
    let mut post = echo.clone();
    post["data"] = json!({"name": "post"});
    let mut sending = request(
        "POST",
        "https://discord.com/api/v9/interactions",
        "/api/v9/interactions",
        &[("content-type", "application/json"), ("authorization", "secret")],
        &post.to_string(),
    );
    let sent = http::intercept_request(&mut sending);
    assert!(sent.changed && sent.response.is_none());
    assert_eq!(sending.url, "https://discord.com/api/v9/channels/100000000000000002/messages");
    let body = String::from_utf8_lossy(&sending.body);
    assert!(body.contains("PNGDATA") && body.contains(r#""filename":"card.png""#));
    assert!(
        sending
            .headers
            .iter()
            .any(|(name, value)| name == "authorization" && value == "secret")
    );
    stop_host();
}

fn slider_settings_and_navigation(work: &Path) {
    use astral_core::settings::navigator;
    let root = work.join("slider-host");
    plugin(
        &root,
        "sample",
        "id = 'astral.slider-test'\nname = 'Slider test'\nversion = '1.0'\nmain = 'main.lua'\n",
        "astral.settings.number('dim', 'Dim', { min = 0, max = 90, step = 5, default = 55 })",
    );
    let handle = developer_settings(&root);
    start_host(&root, handle, None);
    assert_eq!(eval("astral.slider-test", "return astral.settings.values.dim"), "55");
    support::configure_pages(handle, "0.0.1");
    let overview = support::batch::Batch::overview();
    let mut buffer = astral_core::wire::out::AstralWireBuffer {
        bytes: core::ptr::null_mut(),
        length: 0,
    };
    unsafe { astral_core::wire::rewrite::astral_settings_rewrite(overview.0.as_ptr(), overview.0.len(), &raw mut buffer) };
    unsafe { astral_core::wire::rewrite::astral_wire_buffer_free(&raw mut buffer) };
    unsafe extern "C" fn deliver(_: *const u8, _: usize) {}
    unsafe extern "C" fn with_wire(work: navigator::Work, context: *mut core::ffi::c_void) {
        unsafe { work(context) };
    }
    navigator::configure(Some(deliver), Some(with_wire));
    navigator::tap("settings.row.astral.plugin-settings:astral.slider-test");
    assert!(navigator::open());
    navigator::slide_position("astral.slider.18.number.dim", 13.0);
    assert_eq!(eval("astral.slider-test", "return astral.settings.values.dim"), "65");
    navigator::slide_position("astral.slider.18.number.dim", 100.0);
    assert_eq!(eval("astral.slider-test", "return astral.settings.values.dim"), "90");
    navigator::slide("astral.slider.18.number.dim", f64::NAN);
    assert_eq!(eval("astral.slider-test", "return astral.settings.values.dim"), "90");
    assert!(navigator::back() && !navigator::open());
    navigator::tap("settings.row.astral.plugin-settings:astral.slider-test");
    assert_ne!(unsafe { astral_core::wire::pages::astral_page_pop(&raw mut buffer) }, 0);
    unsafe { astral_core::wire::rewrite::astral_wire_buffer_free(&raw mut buffer) };
    assert!(!navigator::open() && !navigator::back());
    stop_host();
    support::reset_pages();
}

fn text_settings(work: &Path) {
    use astral_core::settings::pages::{action, model};
    let root = work.join("text-settings-host");
    plugin(
        &root,
        "sample",
        "id = 'astral.text-setting-test'\nname = 'Text setting test'\nversion = '1.0'\nmain = 'main.lua'\n",
        "astral.settings.text('rules', 'Rules', 'first => second', { max_length = 64, section = 'Rules' })\nlocal choices = {}\nfor index = 1, 40 do choices[index] = astral.settings.option('lang-' .. index, 'Language ' .. index) end\nastral.settings.choice('language', 'Language', choices, 'lang-40')\nastral.settings.color('tint', 'Tint', '', { section = 'Rules' })\nastral.settings.on_change('rules', function(value) astral.state.changed = value end)",
    );
    let handle = developer_settings(&root);
    start_host(&root, handle, None);
    let plugin_id = "astral.text-setting-test";
    assert_eq!(eval(plugin_id, "return astral.settings.values.rules"), "first => second");
    assert_eq!(eval(plugin_id, "return astral.time.now_ms() > 0"), "true");
    let plugins = lua_host::plugins_json();
    assert_eq!(plugins.as_array().map(Vec::len), Some(1));
    assert_eq!(plugins[0]["textSettings"][0]["maxLength"], 64);
    assert_eq!(plugins[0]["switches"][0]["options"].as_array().map(Vec::len), Some(40));
    support::on_page("editText", |arguments, reply| {
        assert_eq!(
            (text_at(arguments, 0), text_at(arguments, 1), arguments[2].as_u64()),
            ("Rules".to_owned(), "first => second".to_owned(), Some(64))
        );
        if let Some(reply) = reply {
            reply(Some("first => third".to_owned()));
        }
        Value::Null
    });
    support::configure_pages(handle, "0.0.1");
    let route = format!("plugin-settings:{plugin_id}");
    let page = model(&route);
    assert_eq!(page["sections"][1]["rows"][39]["key"], "choice.language.lang-40");
    assert_eq!(
        (
            page["sections"][0]["rows"][1]["key"].as_str(),
            page["sections"][0]["rows"][1]["detail"].as_str()
        ),
        (Some("color.tint"), Some("Automatic"))
    );
    assert_eq!(
        action(&route, "color.tint", support::changed()).as_deref(),
        Some("plugin-color:astral.text-setting-test:tint")
    );
    let color_page = model("plugin-color:astral.text-setting-test:tint");
    assert!(
        color_page["sections"][0]["rows"][1]["icon"]
            .as_str()
            .is_some_and(|icon| icon.starts_with("data:image/png;base64,"))
    );
    action("plugin-color:astral.text-setting-test:tint", "swatch.5865F2", support::changed());
    assert_eq!(eval(plugin_id, "return astral.settings.values.tint"), "#5865F2");
    ui::set_text(plugin_id, "tint", "blue");
    assert_eq!(eval(plugin_id, "return astral.settings.values.tint"), "#5865F2");
    action(&route, "choice.language.lang-40", support::changed());
    assert_eq!(eval(plugin_id, "return astral.settings.values.language"), "lang-40");
    assert_eq!(page["sections"][0]["rows"][0]["key"], "text.rules");
    let changed = recorder::<()>();
    let record = changed.clone();
    action(&route, "text.rules", Arc::new(move || push(&record, ())));
    assert_eq!(recorded(&changed).len(), 1);
    assert_eq!(eval(plugin_id, "return astral.settings.values.rules"), "first => third");
    assert_eq!(eval(plugin_id, "return astral.state.changed"), "first => third");
    stop_host();
    support::reset_pages();
}

#[test]
fn lua_host_scenarios() {
    let work = support::work("lua-host");
    network_requests(&work);
    outgoing_message_transforms(&work);
    composer_styles(&work);
    message_background(&work);
    node_patch(&work);
    node_insert(&work);
    bytecode_only_when_signed(&work);
    file_picker_and_dialogs(&work);
    gateway_message_events_and_haptics(&work);
    slider_settings_and_navigation(&work);
    text_settings(&work);
    gateway_and_http_intercepts(&work);
    commands(&work);
}
