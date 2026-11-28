use core::ffi::c_char;

use serde_json::{Value, json};

use super::host::{self, Reply, Request};
use super::reports;
use super::state::{REFRESH_DECORATIONS, signing_key};
use crate::support::bridge::{AstralRustString, bytes};

const PERMILLE: f64 = 1000.0;

#[derive(Clone, Copy)]
enum Method {
    Has,
    ShowToast,
    CloseToast,
    ShowTooltip,
    ShowAlert,
    WriteClipboard,
    ReadClipboard,
    OpenUrl,
    TriggerHaptic,
    Speak,
    PlaySound,
    StopAudio,
    Notify,
    ScrollToMessage,
    FocusComposer,
    InsertComposerText,
    RefreshDecorations,
    Environment,
    DeveloperLog,
    RecordFailure,
    VerifySignature,
    NetworkRequest,
    DiscordRequest,
    CancelNetworkRequest,
    Confirm,
    OpenSheet,
    UpdateSheet,
    CloseSheet,
    PressNode,
    RenderImage,
    PickFile,
    CancelFilePick,
    WebAuth,
    CancelWebAuth,
    DispatchGateway,
    PickedMedia,
    PickedVideoUrl,
    KeepAlive,
    PushRegistered,
    OfferPick,
    DismissSheets,
}

impl Method {
    fn parse(name: &str) -> Option<Method> {
        Some(match name {
            "has" => Method::Has,
            "showToast" => Method::ShowToast,
            "closeToast" => Method::CloseToast,
            "showTooltip" => Method::ShowTooltip,
            "showAlert" => Method::ShowAlert,
            "writeClipboard" => Method::WriteClipboard,
            "readClipboard" => Method::ReadClipboard,
            "openURL" => Method::OpenUrl,
            "triggerHaptic" => Method::TriggerHaptic,
            "speak" => Method::Speak,
            "playSound" => Method::PlaySound,
            "stopAudio" => Method::StopAudio,
            "notify" => Method::Notify,
            "scrollToMessage" => Method::ScrollToMessage,
            "focusComposer" => Method::FocusComposer,
            "insertComposerText" => Method::InsertComposerText,
            "refreshDecorations" => Method::RefreshDecorations,
            "environment" => Method::Environment,
            "developerLog" => Method::DeveloperLog,
            "recordFailure" => Method::RecordFailure,
            "verifySignature" => Method::VerifySignature,
            "networkRequest" => Method::NetworkRequest,
            "discordRequest" => Method::DiscordRequest,
            "cancelNetworkRequest" => Method::CancelNetworkRequest,
            "confirm" => Method::Confirm,
            "openSheet" => Method::OpenSheet,
            "updateSheet" => Method::UpdateSheet,
            "closeSheet" => Method::CloseSheet,
            "pressNode" => Method::PressNode,
            "renderImage" => Method::RenderImage,
            "pickFile" => Method::PickFile,
            "cancelFilePick" => Method::CancelFilePick,
            "webAuth" => Method::WebAuth,
            "cancelWebAuth" => Method::CancelWebAuth,
            "dispatchGateway" => Method::DispatchGateway,
            "pickedMedia" => Method::PickedMedia,
            "pickedVideoURL" => Method::PickedVideoUrl,
            "keepAlive" => Method::KeepAlive,
            "pushRegistered" => Method::PushRegistered,
            "offerPick" => Method::OfferPick,
            "dismissSheets" => Method::DismissSheets,
            _ => return None,
        })
    }

    fn advertised(name: &str) -> bool {
        matches!(
            Method::parse(name),
            Some(
                Method::Confirm
                    | Method::OpenSheet
                    | Method::PickFile
                    | Method::WebAuth
                    | Method::NetworkRequest
                    | Method::DiscordRequest
                    | Method::DispatchGateway
            )
        )
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

    fn number(&self, index: usize) -> f64 {
        self.at(index).as_f64().unwrap_or(0.0)
    }

    fn identifier(&self, index: usize) -> u64 {
        let value = self.at(index);
        value
            .as_u64()
            .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
            .unwrap_or(0)
    }
}

fn value(value: Value) -> Value {
    let mut fields = serde_json::Map::new();
    fields.insert("value".to_owned(), value);
    Value::Object(fields)
}

fn done() -> Value {
    value(Value::Null)
}

fn flag(reply: &Reply) -> Value {
    value(json!(reply.flag()))
}

fn optional_text(text: Option<String>) -> Value {
    value(text.map_or(Value::Null, Value::String))
}

fn threw() -> Value {
    json!({ "threw": true })
}

fn missing() -> Value {
    json!({ "missing": true })
}

fn slot_index(slot: &str) -> Option<u64> {
    slot.get(1..)
        .and_then(|digits| digits.parse::<u64>().ok())
        .filter(|index| *index > 0)
        .map(|index| index - 1)
}

fn picked_media(slot: &str) -> Value {
    let Some(index) = slot_index(slot) else { return value(Value::Null) };
    let entry = super::picked_media::entry(index, |limit| {
        host::request(Request::FetchPickedMedia { limit });
    });
    value(entry.as_ref().map_or(Value::Null, super::picked_media::describe))
}

fn picked_video(slot: &str) -> Value {
    let Some(index) = slot_index(slot) else { return value(Value::Null) };
    optional_text(super::picked_media::video_url(index))
}

pub fn picked_media_fetched(entries: &str) {
    if super::picked_media::fetched(entries) {
        refresh_decorations();
    }
}

const REFRESH_RESEND_MS: u64 = 100;

static LAST_REFRESH_REQUEST_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn should_request_refresh(already_pending: bool, last_ms: u64, now_ms: u64) -> bool {
    !already_pending || now_ms.saturating_sub(last_ms) >= REFRESH_RESEND_MS
}

pub fn request_decoration_refresh() {
    refresh_decorations();
}

fn refresh_decorations() {
    use std::sync::atomic::Ordering;
    crate::lua::host::nodes::forget_tags();
    let already_pending = REFRESH_DECORATIONS.swap(true, Ordering::AcqRel);
    let now_ms = u64::try_from(crate::support::bridge::epoch_ms()).unwrap_or_default();
    if should_request_refresh(already_pending, LAST_REFRESH_REQUEST_MS.load(Ordering::Acquire), now_ms) {
        LAST_REFRESH_REQUEST_MS.store(now_ms, Ordering::Release);
        host::request(Request::RefreshDecorations);
    }
}

fn offer_pick(arguments: &Arguments) -> Value {
    let Some(data) = crate::lua::host::install::decode_base64(arguments.text(0)) else {
        return value(json!(false));
    };
    flag(&host::request(Request::OfferPick {
        name: arguments.text(1),
        data: &data,
    }))
}

fn verify_signature(directory: &str, main: &str) -> bool {
    crate::plugins::signatures::verify_directory(signing_key().as_bytes(), directory.as_bytes(), main, |contents| {
        crate::support::crypto::hex(&crate::support::crypto::sha256(contents)).into_bytes()
    })
}

fn render_image(spec: &Value) -> Value {
    let png = crate::render::image::render(spec);
    value(png.map_or(Value::Null, |png| json!(crate::lua::host::install::encode_base64(&png))))
}

fn scaled(value: f64) -> i64 {
    (value * PERMILLE) as i64
}

fn dispatch(method: Method, arguments: &Arguments) -> Value {
    match method {
        Method::Has => value(json!(Method::advertised(arguments.text(0)))),
        Method::ShowToast => {
            let spec = host::json_text(arguments.at(1));
            host::request(Request::ShowToast {
                toast: arguments.identifier(0),
                spec: &spec,
            });
            done()
        }
        Method::CloseToast => {
            host::request(Request::CloseToast {
                toast: arguments.identifier(0),
            });
            done()
        }
        Method::ShowTooltip => {
            host::request(Request::ShowTooltip { text: arguments.text(0) });
            done()
        }
        Method::ShowAlert => {
            host::request(Request::ShowAlert {
                title: arguments.text(0),
                message: arguments.text(1),
            });
            done()
        }
        Method::WriteClipboard => flag(&host::request(Request::WriteClipboard { value: arguments.text(0) })),
        Method::ReadClipboard => optional_text(host::request(Request::ReadClipboard).into_text()),
        Method::OpenUrl => flag(&host::request(Request::OpenUrl { url: arguments.text(0) })),
        Method::TriggerHaptic => flag(&host::request(Request::TriggerHaptic {
            strength: arguments.text(0),
        })),
        Method::Speak => flag(&host::request(Request::Speak {
            rate_permille: scaled(arguments.number(2)),
            text: arguments.text(0),
            language: arguments.text(1),
        })),
        Method::PlaySound => flag(&host::request(Request::PlaySound {
            volume_permille: scaled(arguments.number(1)),
            path: arguments.text(0),
        })),
        Method::StopAudio => {
            host::request(Request::StopAudio);
            done()
        }
        Method::Notify => {
            let payload = host::json_text(&json!({
                "body": arguments.text(2),
                "link": arguments.text(4),
                "reply": arguments.at(5),
            }));
            flag(&host::request(Request::Notify {
                delay_ms: scaled(arguments.number(3)),
                title: arguments.text(1),
                payload: &payload,
            }))
        }
        Method::ScrollToMessage => {
            let align = std::ffi::CString::new(arguments.text(2)).unwrap_or_default();
            let scrolled = unsafe {
                crate::wire::commands::astral_wire_scroll_to_message(
                    arguments.identifier(0),
                    core::ffi::c_int::from(arguments.flag(1)),
                    align.as_ptr(),
                )
            } != 0;
            if scrolled {
                refresh_decorations();
            }
            value(json!(scrolled))
        }
        Method::FocusComposer => {
            let focused = crate::wire::commands::astral_wire_focus_composer(core::ffi::c_int::from(arguments.flag(0))) != 0;
            if focused {
                refresh_decorations();
            }
            value(json!(focused))
        }
        Method::InsertComposerText => flag(&host::request(Request::InsertComposerText {
            text: arguments.text(0),
            replace: arguments.flag(1),
        })),
        Method::RefreshDecorations => {
            refresh_decorations();
            done()
        }
        Method::Environment => value(reports::environment()),
        Method::DeveloperLog => {
            host::log(arguments.text(0));
            crate::support::dev_server::append_log(arguments.text(0));
            done()
        }
        Method::RecordFailure => {
            reports::record_plugin_failure(arguments.text(0), arguments.text(1), arguments.text(2));
            done()
        }
        Method::VerifySignature => value(json!(verify_signature(arguments.text(0), arguments.text(1)))),
        Method::NetworkRequest => {
            let complete: fn(u64, &Value) = crate::lua::host::complete_network_request;
            if crate::network::http::start(arguments.identifier(0), arguments.at(1).clone(), complete) {
                done()
            } else {
                threw()
            }
        }
        Method::DiscordRequest => {
            let options = arguments.at(1);
            let text = |name: &str| options.get(name).and_then(Value::as_str).unwrap_or_default();
            if super::discord::send(arguments.identifier(0), text("method"), text("path"), text("body")) {
                done()
            } else {
                threw()
            }
        }
        Method::CancelNetworkRequest => {
            crate::network::http::cancel(arguments.identifier(0));
            done()
        }
        Method::Confirm => {
            let spec = host::json_text(arguments.at(1));
            flag(&host::request(Request::Confirm {
                request: arguments.identifier(0),
                spec: &spec,
            }))
        }
        Method::OpenSheet => {
            let spec = host::json_text(arguments.at(1));
            flag(&host::request(Request::OpenSheet {
                sheet: arguments.identifier(0),
                spec: &spec,
            }))
        }
        Method::UpdateSheet => {
            let spec = host::json_text(arguments.at(1));
            host::request(Request::UpdateSheet {
                sheet: arguments.identifier(0),
                spec: &spec,
            });
            done()
        }
        Method::CloseSheet => {
            host::request(Request::CloseSheet {
                sheet: arguments.identifier(0),
            });
            done()
        }
        Method::PressNode => flag(&host::request(Request::PressNode {
            identifier: arguments.text(0),
            prefix: arguments.flag(1),
        })),
        Method::RenderImage => render_image(arguments.at(0)),
        Method::PickFile => {
            let extensions: Vec<&str> = arguments.at(1).as_array().into_iter().flatten().filter_map(Value::as_str).collect();
            let joined = extensions.join("\n");
            flag(&host::request(Request::PickFile {
                request: arguments.identifier(0),
                extensions: &joined,
            }))
        }
        Method::CancelFilePick => {
            host::request(Request::CancelFilePick {
                request: arguments.identifier(0),
            });
            done()
        }
        Method::WebAuth => {
            super::web_auth::begin(arguments.identifier(0), arguments.text(2));
            flag(&host::request(Request::WebAuth {
                request: arguments.identifier(0),
                url: arguments.text(1),
                callback_url: arguments.text(2),
            }))
        }
        Method::CancelWebAuth => {
            host::request(Request::CancelWebAuth {
                request: arguments.identifier(0),
            });
            done()
        }
        Method::DispatchGateway => flag(&host::request(Request::DispatchGateway {
            message: arguments.text(0),
        })),
        Method::PickedMedia => picked_media(arguments.text(0)),
        Method::PickedVideoUrl => picked_video(arguments.text(0)),
        Method::KeepAlive => flag(&host::request(Request::KeepAlive { on: arguments.flag(0) })),
        Method::PushRegistered => value(json!(push_registered())),
        Method::OfferPick => offer_pick(arguments),
        Method::DismissSheets => flag(&host::request(Request::DismissSheets)),
    }
}

pub unsafe extern "C" fn platform_call(
    method: *const c_char,
    method_length: usize,
    arguments: *const c_char,
    arguments_length: usize,
) -> AstralRustString {
    let name = String::from_utf8_lossy(bytes(method, method_length)).into_owned();
    let parsed: Value = serde_json::from_slice(bytes(arguments, arguments_length)).unwrap_or(Value::Null);
    let reply = Method::parse(&name).map_or_else(missing, |method| dispatch(method, &Arguments(&parsed)));
    AstralRustString::new(host::json_text(&reply).into_bytes())
}

fn push_registered() -> bool {
    let facts: Value = host::request(Request::EnvironmentFacts)
        .into_text()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null);
    facts
        .get("push")
        .and_then(|push| push.get("registered"))
        .and_then(Value::as_bool)
        .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refreshes_coalesce_until_the_pending_one_is_taken() {
        assert!(should_request_refresh(false, 1000, 1001));
        assert!(!should_request_refresh(true, 1000, 1050));
        assert!(should_request_refresh(true, 1000, 1000 + REFRESH_RESEND_MS));
    }
}
