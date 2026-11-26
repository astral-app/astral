use core::ffi::{CStr, c_char};

use serde_json::{Value, json};

use super::host;
use super::state::{App, PackKind, app};
use super::{reload_enabled_plugins, select_pack, theme};
use crate::bridge::{AstralRustString, bytes};
use crate::host_callback::{AstralCallback, adopt_reply};
use crate::lua_host;

const OK: i64 = crate::http_status::OK;
const BAD_REQUEST: i64 = 400;
const IMPORTED_THEME: &str = "Imported";

#[derive(Clone, Copy)]
enum Method {
    Enabled,
    OnMain,
    Status,
    Routes,
    Plugins,
    Reload,
    Eval,
    SetEnabled,
}

impl Method {
    fn parse(name: &str) -> Option<Method> {
        Some(match name {
            "enabled" => Method::Enabled,
            "onMain" => Method::OnMain,
            "status" => Method::Status,
            "routes" => Method::Routes,
            "plugins" => Method::Plugins,
            "reload" => Method::Reload,
            "eval" => Method::Eval,
            "setEnabled" => Method::SetEnabled,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Route {
    Themes,
    UnboundTheme,
    Theme,
    IconPack,
    EmojiPack,
    Reload,
    ExportBackup,
    ImportBackup,
    Gateway,
}

impl Route {
    fn parse(method: &str, route: &str) -> Option<Route> {
        Some(match (method, route) {
            ("GET", "/themes") => Route::Themes,
            ("POST", "/unbound-theme") => Route::UnboundTheme,
            ("POST", "/theme") => Route::Theme,
            ("POST", "/icon-pack") => Route::IconPack,
            ("POST", "/emoji-pack") => Route::EmojiPack,
            ("POST", "/reload") => Route::Reload,
            ("GET", "/backup") => Route::ExportBackup,
            ("POST", "/backup") => Route::ImportBackup,
            ("POST", "/gateway") => Route::Gateway,
            _ => return None,
        })
    }
}

fn response(status: i64, body: Value) -> Value {
    let mut fields = serde_json::Map::new();
    fields.insert("status".to_owned(), json!(status));
    fields.insert("body".to_owned(), body);
    Value::Object(fields)
}

fn route(current: &App, request: &Value) -> Value {
    let text = |key: &str| request.get(key).and_then(Value::as_str).unwrap_or_default().to_owned();
    let Some(route) = Route::parse(&text("method"), &text("route")) else {
        return Value::Null;
    };
    let body = text("body");
    let parsed: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    match route {
        Route::Themes => {
            let themes = theme::list(current);
            let identifiers: Vec<Value> = themes
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|theme| theme.get("id").cloned())
                .collect();
            response(OK, json!({ "active": theme::active(current), "themes": identifiers }))
        }
        Route::UnboundTheme => {
            if let Some(error) = theme::import_unbound(current, &body, IMPORTED_THEME) {
                return response(BAD_REQUEST, json!({ "error": error }));
            }
            theme::apply(current, &theme::active(current));
            super::refresh_settings();
            response(OK, json!({ "active": theme::active(current) }))
        }
        Route::Theme | Route::IconPack | Route::EmojiPack => {
            let Some(identifier) = parsed.get("id").and_then(Value::as_str).map(str::to_owned) else {
                return response(BAD_REQUEST, json!({ "error": "id is required" }));
            };
            match route {
                Route::IconPack if !select_pack(current, PackKind::Icons, &identifier) => {
                    return response(BAD_REQUEST, json!({ "error": "no such icon pack" }));
                }
                Route::EmojiPack if !current.select_pack(PackKind::Emoji, &identifier) => {
                    return response(BAD_REQUEST, json!({ "error": "no such emoji pack" }));
                }
                Route::Theme => theme::apply(current, &identifier),
                _ => {}
            }
            super::refresh_settings();
            let active = if route == Route::Theme {
                theme::active(current)
            } else {
                identifier
            };
            response(OK, json!({ "active": active }))
        }
        Route::Reload => {
            let reloaded = reload_enabled_plugins();
            current.invalidate_packs();
            super::refresh_images(current);
            theme::apply(current, &theme::active(current));
            super::refresh_settings();
            response(OK, json!({ "plugins": reloaded, "theme": theme::active(current) }))
        }
        Route::ExportBackup => response(OK, json!({ "backup": theme::export_backup(current) })),
        Route::ImportBackup => {
            let Some(backup) = parsed.get("backup").and_then(Value::as_str) else {
                return response(BAD_REQUEST, json!({ "error": "backup is required" }));
            };
            if let Some(error) = theme::apply_backup(current, backup) {
                return response(BAD_REQUEST, json!({ "error": error }));
            }
            super::refresh_settings();
            response(OK, json!({ "imported": true }))
        }
        Route::Gateway => {
            let queued = !body.is_empty() && host::request(host::Request::InjectGateway { message: &body }).flag();
            let status = if queued { OK } else { BAD_REQUEST };
            response(
                status,
                json!({ "queued": queued, "needs": "Staff DevTools on, for an uncompressed gateway" }),
            )
        }
    }
}

fn status() -> Value {
    let stats = crate::wire::decorate::astral_wire_stats();
    let pages = crate::wire::pages::astral_page_template_status();
    let pages = if pages.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(pages) }.to_string_lossy().into_owned()
    };
    json!({
        "network": super::network::stats(),
        "wire": {
            "channel": crate::wire::ids::astral_wire_current_channel().to_string(),
            "authors": stats.authors,
            "withUser": stats.with_user,
            "withChannel": stats.with_channel,
            "tagged": stats.tagged,
            "pages": pages,
            "navigators": crate::wire::pages::astral_page_navigator_count(),
            "depth": crate::wire::pages::astral_page_depth(),
        }
    })
}

fn dispatch(current: &App, method: Method, arguments: &Value, callback: AstralCallback) -> Value {
    let text = |index: usize| arguments.get(index).and_then(Value::as_str).unwrap_or_default().to_owned();
    match method {
        Method::Enabled => json!(current.flag("astral", "developer-mode", false)),
        Method::OnMain => {
            if let Some(reply) = adopt_reply(callback) {
                reply(None);
            }
            Value::Null
        }
        Method::Status => status(),
        Method::Routes => route(current, arguments),
        Method::Plugins => lua_host::plugins_json(),
        Method::Reload => {
            lua_host::reload(&text(0));
            Value::Null
        }
        Method::Eval => json!(lua_host::eval(&text(0), &text(1))),
        Method::SetEnabled => {
            lua_host::set_enabled(&text(0), arguments.get(1).and_then(Value::as_bool).unwrap_or(false));
            Value::Null
        }
    }
}

unsafe extern "C" fn dev_host_call(
    method: *const c_char,
    method_length: usize,
    arguments: *const c_char,
    arguments_length: usize,
    callback: AstralCallback,
) -> AstralRustString {
    let empty = AstralRustString {
        bytes: core::ptr::null_mut(),
        length: 0,
    };
    let Some(current) = app() else {
        drop(adopt_reply(callback));
        return empty;
    };
    let name = String::from_utf8_lossy(bytes(method, method_length)).into_owned();
    let parsed: Value = serde_json::from_slice(bytes(arguments, arguments_length)).unwrap_or(Value::Null);
    let result = match Method::parse(&name) {
        Some(method) => dispatch(current, method, &parsed, callback),
        None => {
            drop(adopt_reply(callback));
            Value::Null
        }
    };
    if result.is_null() {
        return empty;
    }
    AstralRustString::new(host::json_text(&result).into_bytes())
}

pub fn start(current: &App) {
    crate::dev_server::start(
        Some(dev_host_call),
        current.settings_handle(),
        current.version.clone(),
        current.state.clone(),
    );
}
