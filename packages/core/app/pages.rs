use core::ffi::{c_char, c_void};
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

use super::host::{self, Request};
use super::state::{App, PackKind, app, wire};
use super::{reports, select_pack, soft_restart, theme};
use crate::lua::host as lua_host;
use crate::settings::navigator::Work;
use crate::support::bridge::{AstralRustString, bytes};
use crate::support::host_callback::{AstralCallback, Changed, Reply, adopt_reply};

const READY: &str = "ready";

static TEXT_EDITS: Mutex<BTreeMap<u64, Reply>> = Mutex::new(BTreeMap::new());
static NEXT_TEXT_EDIT: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy)]
enum Method {
    Capabilities,
    Context,
    Plugins,
    SetEnabled,
    SetSwitch,
    SetChoice,
    SetNumber,
    SetText,
    PressSettingsButton,
    ResetSettings,
    RemoveStorage,
    Remove,
    Rescan,
    PairingCode,
    CrashReports,
    BlockedRequests,
    Themes,
    ActiveTheme,
    IconPacks,
    ActiveIconPack,
    EmojiPacks,
    FontPacks,
    ActiveFontPack,
    SelectFontPack,
    ActiveEmojiPack,
    EmojiAnimated,
    SyncFolder,
    ToggleSync,
    AppIcons,
    SetAppIcon,
    Catalog,
    CatalogState,
    CatalogError,
    CatalogUrl,
    ApplyTheme,
    SelectIconPack,
    SelectEmojiPack,
    SetEmojiAnimated,
    InstallStatus,
    InstallPackage,
    RefreshCatalog,
    AuthorAvatar,
    PluginIconSvg,
    EditText,
    ExportBackup,
    ImportBackup,
    SoftRestart,
    RestartGatewaySession,
    OpenReport,
    Notify,
}

impl Method {
    fn parse(name: &str) -> Option<Method> {
        Some(match name {
            "capabilities" => Method::Capabilities,
            "context" => Method::Context,
            "plugins" => Method::Plugins,
            "setEnabled" => Method::SetEnabled,
            "setSwitch" => Method::SetSwitch,
            "setChoice" => Method::SetChoice,
            "setNumber" => Method::SetNumber,
            "setText" => Method::SetText,
            "pressSettingsButton" => Method::PressSettingsButton,
            "resetSettings" => Method::ResetSettings,
            "removeStorage" => Method::RemoveStorage,
            "remove" => Method::Remove,
            "rescan" => Method::Rescan,
            "pairingCode" => Method::PairingCode,
            "crashReports" => Method::CrashReports,
            "blockedRequests" => Method::BlockedRequests,
            "themes" => Method::Themes,
            "activeTheme" => Method::ActiveTheme,
            "iconPacks" => Method::IconPacks,
            "activeIconPack" => Method::ActiveIconPack,
            "emojiPacks" => Method::EmojiPacks,
            "fontPacks" => Method::FontPacks,
            "activeFontPack" => Method::ActiveFontPack,
            "selectFontPack" => Method::SelectFontPack,
            "activeEmojiPack" => Method::ActiveEmojiPack,
            "emojiAnimated" => Method::EmojiAnimated,
            "syncFolder" => Method::SyncFolder,
            "toggleSync" => Method::ToggleSync,
            "appIcons" => Method::AppIcons,
            "setAppIcon" => Method::SetAppIcon,
            "catalog" => Method::Catalog,
            "catalogState" => Method::CatalogState,
            "catalogError" => Method::CatalogError,
            "catalogURL" => Method::CatalogUrl,
            "applyTheme" => Method::ApplyTheme,
            "selectIconPack" => Method::SelectIconPack,
            "selectEmojiPack" => Method::SelectEmojiPack,
            "setEmojiAnimated" => Method::SetEmojiAnimated,
            "installStatus" => Method::InstallStatus,
            "installPackage" => Method::InstallPackage,
            "refreshCatalog" => Method::RefreshCatalog,
            "authorAvatar" => Method::AuthorAvatar,
            "pluginIconSVG" => Method::PluginIconSvg,
            "editText" => Method::EditText,
            "exportBackup" => Method::ExportBackup,
            "importBackup" => Method::ImportBackup,
            "softRestart" => Method::SoftRestart,
            "restartGatewaySession" => Method::RestartGatewaySession,
            "openReport" => Method::OpenReport,
            "notify" => Method::Notify,
            _ => return None,
        })
    }
}

struct Arguments<'a>(&'a Value);

impl Arguments<'_> {
    fn at(&self, index: usize) -> &Value {
        self.0.get(index).unwrap_or(&Value::Null)
    }

    fn text(&self, index: usize) -> &str {
        self.at(index).as_str().unwrap_or_default()
    }

    fn flag(&self, index: usize) -> bool {
        self.at(index).as_bool().unwrap_or(false)
    }

    fn count(&self, index: usize) -> usize {
        self.at(index).as_u64().and_then(|count| usize::try_from(count).ok()).unwrap_or(0)
    }
}

fn marketplace_text(produce: extern "C" fn() -> AstralRustString) -> Value {
    json!(super::ffi::take(produce()))
}

fn changed_from(reply: Option<Reply>) -> Changed {
    std::sync::Arc::new(move || {
        if let Some(reply) = &reply {
            reply(None);
        }
    })
}

fn edit_text(title: &str, value: &str, maximum: usize, reply: Option<Reply>) {
    let Some(reply) = reply else { return };
    let mut request = NEXT_TEXT_EDIT.fetch_add(1, Ordering::Relaxed);
    if request == 0 {
        request = NEXT_TEXT_EDIT.fetch_add(1, Ordering::Relaxed);
    }
    TEXT_EDITS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(request, reply.clone());
    let maximum = i32::try_from(maximum).unwrap_or(i32::MAX);
    let opened = host::request(Request::EditText {
        request,
        title,
        value,
        maximum,
    })
    .flag();
    if !opened {
        TEXT_EDITS.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).remove(&request);
        reply(None);
    }
}

pub fn complete_text_edit(request: u64, value: Option<String>) {
    if request == 0 {
        return;
    }
    let reply = TEXT_EDITS.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).remove(&request);
    if let Some(reply) = reply {
        reply(value);
    }
}

fn dispatch(current: &App, method: Method, arguments: &Arguments, reply: Option<Reply>) -> Value {
    match method {
        Method::Capabilities => json!({
            "restartGatewaySession": true,
            "editText": true,
            "installPackage": true,
            "refreshCatalog": true,
        }),
        Method::Context => json!({
            "recovering": current.recovering(),
            "appCode": super::guest_source::current().map_or("", |source| if source.bundled { "built-in" } else { "ota" }),
            "discordVersion": current.discord_version,
            "compatibility": READY,
        }),
        Method::Plugins => lua_host::plugins_json(),
        Method::SetEnabled => {
            lua_host::set_enabled(arguments.text(0), arguments.flag(1));
            Value::Null
        }
        Method::SetSwitch => {
            crate::lua::host::ui::set_switch(arguments.text(0), arguments.text(1), arguments.flag(2));
            Value::Null
        }
        Method::SetChoice => {
            crate::lua::host::ui::set_choice(arguments.text(0), arguments.text(1), arguments.text(2));
            Value::Null
        }
        Method::SetNumber => {
            crate::lua::host::ui::set_number(arguments.text(0), arguments.text(1), arguments.at(2).as_f64().unwrap_or(0.0));
            Value::Null
        }
        Method::SetText => {
            crate::lua::host::ui::set_text(arguments.text(0), arguments.text(1), arguments.text(2));
            Value::Null
        }
        Method::PressSettingsButton => {
            crate::lua::host::ui::press_settings_button(arguments.text(0), arguments.text(1));
            Value::Null
        }
        Method::ResetSettings => {
            lua_host::reset_settings(arguments.text(0));
            Value::Null
        }
        Method::RemoveStorage => {
            crate::lua::host::api::remove_storage(arguments.text(0));
            Value::Null
        }
        Method::Remove => {
            lua_host::remove(arguments.text(0));
            Value::Null
        }
        Method::Rescan => {
            lua_host::rescan();
            Value::Null
        }
        Method::PairingCode => json!(crate::support::dev_server::pairing_code()),
        Method::CrashReports => reports::plugin_reports(),
        Method::BlockedRequests => json!(current.blocked_requests()),
        Method::Themes => theme::list(current),
        Method::ActiveTheme => json!(theme::active(current)),
        Method::IconPacks => current.packs(PackKind::Icons),
        Method::ActiveIconPack => json!(current.active_pack(PackKind::Icons)),
        Method::EmojiPacks => current.packs(PackKind::Emoji),
        Method::FontPacks => current.packs(PackKind::Fonts),
        Method::ActiveFontPack => json!(current.active_pack(PackKind::Fonts)),
        Method::ActiveEmojiPack => json!(current.active_pack(PackKind::Emoji)),
        Method::EmojiAnimated => json!(current.emoji_animated()),
        Method::SyncFolder => json!(super::sync::folder().unwrap_or_default()),
        Method::ToggleSync => {
            if super::sync::folder().is_some() {
                super::sync::forget();
            } else {
                super::sync::choose();
            }
            Value::Null
        }
        Method::AppIcons => match host::request(Request::AppIcons) {
            host::Reply::Text(Some(text)) => serde_json::from_str(&text).unwrap_or(Value::Null),
            _ => Value::Null,
        },
        Method::SetAppIcon => json!(host::request(Request::SetAppIcon { name: arguments.text(0) }).flag()),
        Method::Catalog => crate::plugins::marketplace::current_catalog(),
        Method::CatalogState => marketplace_text(crate::plugins::marketplace::astral_marketplace_state),
        Method::CatalogError => marketplace_text(crate::plugins::marketplace::astral_marketplace_error),
        Method::CatalogUrl => marketplace_text(crate::plugins::marketplace::astral_marketplace_url),
        Method::ApplyTheme => {
            theme::apply(current, arguments.text(0));
            Value::Null
        }
        Method::SelectIconPack => {
            select_pack(current, PackKind::Icons, arguments.text(0));
            Value::Null
        }
        Method::SelectFontPack => {
            select_pack(current, PackKind::Fonts, arguments.text(0));
            Value::Null
        }
        Method::SelectEmojiPack => {
            select_pack(current, PackKind::Emoji, arguments.text(0));
            Value::Null
        }
        Method::SetEmojiAnimated => {
            current.set_emoji_animated(arguments.flag(0));
            host::request(Request::RefreshEmoji);
            Value::Null
        }
        Method::InstallStatus => {
            let (pointer, length) = super::ffi::raw(arguments.text(0));
            super::ffi::take_optional(unsafe { crate::plugins::marketplace::astral_marketplace_install_status(pointer, length) })
                .map_or(Value::Null, Value::String)
        }
        Method::InstallPackage => {
            theme::marketplace_install(arguments.at(0), theme::PackageKind::parse(arguments.text(1)), changed_from(reply));
            Value::Null
        }
        Method::RefreshCatalog => {
            theme::marketplace_refresh(changed_from(reply));
            Value::Null
        }
        Method::AuthorAvatar => json!(theme::author_avatar(arguments.text(0), arguments.count(1))),
        Method::PluginIconSvg => theme::plugin_icon_uri(arguments.at(0)).map_or(Value::Null, Value::String),
        Method::EditText => {
            edit_text(arguments.text(0), arguments.text(1), arguments.count(2), reply);
            Value::Null
        }
        Method::ExportBackup => {
            host::request(Request::ExportBackup);
            Value::Null
        }
        Method::ImportBackup => {
            host::request(Request::ImportBackup);
            Value::Null
        }
        Method::SoftRestart => {
            soft_restart(current);
            Value::Null
        }
        Method::RestartGatewaySession => {
            host::request(Request::RestartGatewaySession);
            Value::Null
        }
        Method::OpenReport => {
            reports::open_report(arguments.count(0));
            Value::Null
        }
        Method::Notify => {
            host::request(Request::ShowToast {
                title: arguments.text(0),
                message: arguments.text(1),
            });
            Value::Null
        }
    }
}

unsafe extern "C" fn pages_host_call(
    method: *const c_char,
    method_length: usize,
    arguments: *const c_char,
    arguments_length: usize,
    callback: AstralCallback,
) -> AstralRustString {
    let reply = adopt_reply(callback);
    let Some(current) = app() else {
        return AstralRustString::new(Vec::new());
    };
    let name = String::from_utf8_lossy(bytes(method, method_length)).into_owned();
    let parsed: Value = serde_json::from_slice(bytes(arguments, arguments_length)).unwrap_or(Value::Null);
    let result = Method::parse(&name).map_or(Value::Null, |method| dispatch(current, method, &Arguments(&parsed), reply));
    if result.is_null() {
        return AstralRustString {
            bytes: core::ptr::null_mut(),
            length: 0,
        };
    }
    AstralRustString::new(host::json_text(&result).into_bytes())
}

unsafe extern "C" fn deliver_navigator(bytes: *const u8, length: usize) {
    if bytes.is_null() || length == 0 {
        return;
    }
    host::deliver(unsafe { core::slice::from_raw_parts(bytes, length) });
}

unsafe extern "C" fn with_wire(work: Work, context: *mut c_void) {
    let _held = wire();
    unsafe { work(context) };
}

pub fn configure(current: &App) {
    crate::settings::pages::configure(Some(pages_host_call), current.settings_handle(), current.version.clone());
    crate::settings::navigator::configure(Some(deliver_navigator), Some(with_wire));
    refresh_catalog_if_due(true);
}

pub fn refresh_catalog_if_due(force: bool) {
    crate::settings::pages::refresh_catalog_if_due(super::now_seconds(), force, std::sync::Arc::new(super::refresh_settings));
}
