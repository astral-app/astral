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
    Reactors,
    KeepAlive,
    OfferPick,
    DismissSheets,
}

impl Method {
    fn parse(name: &str) -> Option<Method> {
        Some(match name {
            "has" => Method::Has,
            "showToast" => Method::ShowToast,
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
            "reactors" => Method::Reactors,
            "keepAlive" => Method::KeepAlive,
            "offerPick" => Method::OfferPick,
            "dismissSheets" => Method::DismissSheets,
            _ => return None,
        })
    }

    fn advertised(name: &str) -> bool {
        matches!(
            Method::parse(name),
            Some(
                Method::Confirm | Method::OpenSheet | Method::PickFile | Method::WebAuth | Method::NetworkRequest | Method::DispatchGateway
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
    let Some(described) = host::request(Request::PickedMedia { index }).into_text() else {
        return value(Value::Null);
    };
    let mut lines = described.splitn(3, '\n');
    let (Some(size), Some(duration), Some(kind)) = (lines.next(), lines.next(), lines.next()) else {
        return value(Value::Null);
    };
    value(json!({
        "size": size.trim().parse::<i64>().unwrap_or(0),
        "duration": duration.trim().parse::<f64>().unwrap_or(0.0),
        "type": kind,
    }))
}

fn picked_video(slot: &str) -> Value {
    let Some(index) = slot_index(slot) else { return value(Value::Null) };
    optional_text(host::request(Request::PickedVideo { index }).into_text())
}

fn refresh_decorations() {
    REFRESH_DECORATIONS.store(true, std::sync::atomic::Ordering::Release);
    host::request(Request::RefreshDecorations);
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

fn ensure_session_token() {
    if !crate::discord::session::current().is_empty() {
        return;
    }
    if let Some(token) = host::request(Request::SessionToken).into_text() {
        super::set_session_token(&token);
    }
}

fn reactors(arguments: &Arguments) -> Value {
    ensure_session_token();
    let (users, fresh) = crate::discord::reactors::lookup(
        arguments.text(0),
        arguments.text(1),
        arguments.text(2),
        arguments.flag(3),
        arguments.identifier(4),
    );
    value(json!({ "users": users.unwrap_or(Value::Null), "fresh": fresh }))
}

fn render_image(spec: &Value) -> Value {
    let png = host::request(Request::RenderImage {
        spec: &host::json_text(spec),
    })
    .into_bytes();
    value(png.map_or(Value::Null, |png| json!(crate::lua::host::install::encode_base64(&png))))
}

fn scaled(value: f64) -> i64 {
    (value * PERMILLE) as i64
}

fn dispatch(method: Method, arguments: &Arguments) -> Value {
    match method {
        Method::Has => value(json!(Method::advertised(arguments.text(0)))),
        Method::ShowToast => {
            host::request(Request::ShowToast {
                title: arguments.text(0),
                message: arguments.text(1),
            });
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
            let payload = host::json_text(&json!({ "body": arguments.text(2), "link": arguments.text(4) }));
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
            let options = host::json_text(arguments.at(1));
            match host::request(Request::NetworkRequest {
                request: arguments.identifier(0),
                options: &options,
            }) {
                Reply::Done | Reply::Flag(true) => done(),
                _ => threw(),
            }
        }
        Method::CancelNetworkRequest => match host::request(Request::CancelNetworkRequest {
            request: arguments.identifier(0),
        }) {
            Reply::Done | Reply::Flag(true) => done(),
            _ => threw(),
        },
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
        Method::WebAuth => flag(&host::request(Request::WebAuth {
            request: arguments.identifier(0),
            url: arguments.text(1),
            callback_url: arguments.text(2),
        })),
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
        Method::Reactors => reactors(arguments),
        Method::KeepAlive => flag(&host::request(Request::KeepAlive { on: arguments.flag(0) })),
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
