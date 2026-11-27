use std::collections::HashMap;
use std::ffi::CString;
use std::sync::Mutex;

use astral_core::app::host::{Bridge, Reply, Request};
use jni::JavaVM;
use jni::objects::{GlobalRef, JClass, JObject, JStaticMethodID, JString, JValue, JValueOwned};
use jni::signature::{Primitive, ReturnType};
use jni::sys::jvalue;

const LOG_INFO: core::ffi::c_int = 4;
const LOG_TAG: &core::ffi::CStr = c"astral";
const LOCAL_FRAME: i32 = 16;

unsafe extern "C" {
    fn __android_log_write(
        priority: core::ffi::c_int,
        tag: *const core::ffi::c_char,
        text: *const core::ffi::c_char,
    ) -> core::ffi::c_int;
}

pub fn log(line: &str) {
    let Ok(text) = CString::new(line.replace('\0', " ")) else {
        return;
    };
    unsafe { __android_log_write(LOG_INFO, LOG_TAG.as_ptr(), text.as_ptr()) };
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Method {
    Deliver,
    DiscordRequest,
    ExportBackup,
    ImportBackup,
    OpenReport,
    RefreshEmoji,
    ShowAlert,
    ShowToast,
    ShowTooltip,
    TriggerHaptic,
    CopyText,
    PickFile,
    CancelFilePick,
    OpenWebAuth,
    CancelWebAuth,
    EditText,
    RestartGatewaySession,
    PasteText,
    OpenUrl,
    Notify,
    Speak,
    PlaySound,
    StopAudio,
    FetchPickedMedia,
    DispatchGateway,
    PressNode,
    InsertComposerText,
    ShowConfirm,
    EnvironmentFacts,
    OpenSheet,
    UpdateSheet,
    CloseSheet,
    SyncFolderName,
    SyncChooseFolder,
    SyncForgetFolder,
    SyncRead,
    SyncWrite,
}

#[derive(Clone, Copy)]
enum Returns {
    Nothing,
    Flag,
    Text,
}

impl Method {
    const ALL: [Method; 37] = [
        Method::Deliver,
        Method::DiscordRequest,
        Method::ExportBackup,
        Method::ImportBackup,
        Method::OpenReport,
        Method::RefreshEmoji,
        Method::ShowAlert,
        Method::ShowToast,
        Method::ShowTooltip,
        Method::TriggerHaptic,
        Method::CopyText,
        Method::PickFile,
        Method::CancelFilePick,
        Method::OpenWebAuth,
        Method::CancelWebAuth,
        Method::EditText,
        Method::RestartGatewaySession,
        Method::PasteText,
        Method::OpenUrl,
        Method::Notify,
        Method::Speak,
        Method::PlaySound,
        Method::StopAudio,
        Method::FetchPickedMedia,
        Method::DispatchGateway,
        Method::PressNode,
        Method::InsertComposerText,
        Method::ShowConfirm,
        Method::EnvironmentFacts,
        Method::OpenSheet,
        Method::UpdateSheet,
        Method::CloseSheet,
        Method::SyncFolderName,
        Method::SyncChooseFolder,
        Method::SyncForgetFolder,
        Method::SyncRead,
        Method::SyncWrite,
    ];

    const fn name(self) -> &'static str {
        match self {
            Method::Deliver => "deliver",
            Method::DiscordRequest => "discordRequest",
            Method::ExportBackup => "exportBackup",
            Method::ImportBackup => "importBackup",
            Method::OpenReport => "openReport",
            Method::RefreshEmoji => "refreshEmoji",
            Method::ShowAlert => "showAlert",
            Method::ShowToast => "showToast",
            Method::ShowTooltip => "showTooltip",
            Method::TriggerHaptic => "triggerHaptic",
            Method::CopyText => "copyText",
            Method::PickFile => "pickFile",
            Method::CancelFilePick => "cancelFilePick",
            Method::OpenWebAuth => "openWebAuth",
            Method::CancelWebAuth => "cancelWebAuth",
            Method::EditText => "editText",
            Method::RestartGatewaySession => "restartGatewaySession",
            Method::PasteText => "pasteText",
            Method::OpenUrl => "openURL",
            Method::Notify => "notify",
            Method::Speak => "speak",
            Method::PlaySound => "playSound",
            Method::StopAudio => "stopAudio",
            Method::FetchPickedMedia => "fetchPickedMedia",
            Method::DispatchGateway => "dispatchGateway",
            Method::PressNode => "pressNode",
            Method::InsertComposerText => "insertComposerText",
            Method::ShowConfirm => "showConfirm",
            Method::EnvironmentFacts => "environmentFacts",
            Method::OpenSheet => "openSheet",
            Method::UpdateSheet => "updateSheet",
            Method::CloseSheet => "closeSheet",
            Method::SyncFolderName => "syncFolderName",
            Method::SyncChooseFolder => "syncChooseFolder",
            Method::SyncForgetFolder => "syncForgetFolder",
            Method::SyncRead => "syncRead",
            Method::SyncWrite => "syncWrite",
        }
    }

    const fn signature(self) -> &'static str {
        match self {
            Method::Deliver => "([B)V",
            Method::DiscordRequest => "(JLjava/lang/String;Ljava/lang/String;Ljava/lang/String;)V",
            Method::PickFile | Method::ShowConfirm | Method::OpenSheet | Method::UpdateSheet => {
                "(JLjava/lang/String;)V"
            }
            Method::CancelFilePick
            | Method::CancelWebAuth
            | Method::CloseSheet
            | Method::FetchPickedMedia => "(J)V",
            Method::ExportBackup
            | Method::ImportBackup
            | Method::RefreshEmoji
            | Method::RestartGatewaySession
            | Method::SyncChooseFolder
            | Method::SyncForgetFolder
            | Method::StopAudio => "()V",
            Method::OpenReport
            | Method::TriggerHaptic
            | Method::CopyText
            | Method::OpenUrl
            | Method::ShowTooltip
            | Method::DispatchGateway => "(Ljava/lang/String;)V",
            Method::ShowAlert | Method::ShowToast => "(Ljava/lang/String;Ljava/lang/String;)V",
            Method::OpenWebAuth | Method::Notify | Method::Speak | Method::PlaySound => {
                "(JLjava/lang/String;Ljava/lang/String;)V"
            }
            Method::EditText => "(JLjava/lang/String;Ljava/lang/String;I)V",
            Method::PasteText
            | Method::EnvironmentFacts
            | Method::SyncFolderName
            | Method::SyncRead => "()Ljava/lang/String;",
            Method::SyncWrite => "(Ljava/lang/String;)Z",
            Method::PressNode | Method::InsertComposerText => "(Ljava/lang/String;Z)Z",
        }
    }

    const fn required(self) -> bool {
        !matches!(
            self,
            Method::DispatchGateway
                | Method::PressNode
                | Method::InsertComposerText
                | Method::ShowConfirm
                | Method::EnvironmentFacts
                | Method::OpenSheet
                | Method::UpdateSheet
                | Method::CloseSheet
                | Method::SyncFolderName
                | Method::SyncChooseFolder
                | Method::SyncForgetFolder
                | Method::SyncRead
                | Method::SyncWrite
        )
    }

    const fn returns(self) -> Returns {
        match self {
            Method::PressNode | Method::InsertComposerText | Method::SyncWrite => Returns::Flag,
            Method::PasteText
            | Method::EnvironmentFacts
            | Method::SyncFolderName
            | Method::SyncRead => Returns::Text,
            _ => Returns::Nothing,
        }
    }
}

pub enum Argument<'a> {
    Text(&'a str),
    Long(i64),
    Int(i32),
    Flag(bool),
    Bytes(&'a [u8]),
}

enum Returned {
    Failed,
    Nothing,
    Flag(bool),
    Text(Option<String>),
}

pub struct JniBridge {
    vm: JavaVM,
    class: GlobalRef,
    methods: HashMap<Method, JStaticMethodID>,
    calls: Mutex<()>,
}

impl JniBridge {
    pub fn new(
        vm: JavaVM,
        class: GlobalRef,
        methods: HashMap<Method, JStaticMethodID>,
    ) -> JniBridge {
        JniBridge {
            vm,
            class,
            methods,
            calls: Mutex::new(()),
        }
    }

    pub fn resolve(
        env: &mut jni::JNIEnv,
        class: &JClass,
    ) -> Option<HashMap<Method, JStaticMethodID>> {
        let mut methods = HashMap::new();
        for method in Method::ALL {
            match env.get_static_method_id(class, method.name(), method.signature()) {
                Ok(id) => {
                    methods.insert(method, id);
                }
                Err(_) => {
                    let _ = env.exception_clear();
                    if method.required() {
                        return None;
                    }
                }
            }
        }
        Some(methods)
    }

    fn call(&self, method: Method, arguments: &[Argument]) -> Returned {
        let Some(id) = self.methods.get(&method).copied() else {
            return Returned::Failed;
        };
        let Ok(mut env) = self.vm.attach_current_thread() else {
            return Returned::Failed;
        };
        let outcome = env.with_local_frame(LOCAL_FRAME, |env| -> jni::errors::Result<Returned> {
            let mut objects: Vec<JObject> = Vec::with_capacity(arguments.len());
            for argument in arguments {
                match argument {
                    Argument::Text(text) => objects.push(JObject::from(env.new_string(text)?)),
                    Argument::Bytes(bytes) => {
                        objects.push(JObject::from(env.byte_array_from_slice(bytes)?))
                    }
                    _ => objects.push(JObject::null()),
                }
            }
            let values: Vec<jvalue> = arguments
                .iter()
                .zip(&objects)
                .map(|(argument, object)| match argument {
                    Argument::Text(_) | Argument::Bytes(_) => JValue::Object(object).as_jni(),
                    Argument::Long(value) => JValue::Long(*value).as_jni(),
                    Argument::Int(value) => JValue::Int(*value).as_jni(),
                    Argument::Flag(value) => JValue::Bool(u8::from(*value)).as_jni(),
                })
                .collect();
            let class = JClass::from(env.new_local_ref(self.class.as_obj())?);
            let returns = method.returns();
            let kind = match returns {
                Returns::Nothing => ReturnType::Primitive(Primitive::Void),
                Returns::Flag => ReturnType::Primitive(Primitive::Boolean),
                Returns::Text => ReturnType::Object,
            };
            let result = unsafe { env.call_static_method_unchecked(&class, id, kind, &values) };
            if env.exception_check().unwrap_or(false) {
                let _ = env.exception_clear();
                return Ok(Returned::Failed);
            }
            let Ok(result) = result else {
                return Ok(Returned::Failed);
            };
            Ok(convert(env, returns, result))
        });
        outcome.unwrap_or(Returned::Failed)
    }

    fn locked(&self, method: Method, arguments: &[Argument]) -> Returned {
        let _held = self
            .calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.call(method, arguments)
    }
}

fn convert(env: &mut jni::JNIEnv, returns: Returns, result: JValueOwned) -> Returned {
    match returns {
        Returns::Nothing => Returned::Nothing,
        Returns::Flag => Returned::Flag(result.z().unwrap_or(false)),
        Returns::Text => {
            let object = result.l().unwrap_or_else(|_| JObject::null());
            if object.is_null() {
                return Returned::Text(None);
            }
            let text = JString::from(object);
            Returned::Text(env.get_string(&text).ok().map(String::from))
        }
    }
}

fn succeeded(returned: &Returned) -> Reply {
    Reply::Flag(!matches!(returned, Returned::Failed))
}

fn into_reply(returned: Returned) -> Reply {
    match returned {
        Returned::Failed => Reply::Flag(false),
        Returned::Nothing => Reply::Done,
        Returned::Flag(flag) => Reply::Flag(flag),
        Returned::Text(text) => Reply::Text(text),
    }
}

impl Bridge for JniBridge {
    fn deliver(&self, bytes: &[u8]) {
        self.call(Method::Deliver, &[Argument::Bytes(bytes)]);
    }

    fn log(&self, line: &str) {
        log(line);
    }

    fn request(&self, request: Request) -> Reply {
        match request {
            Request::ShowAlert { title, message } => into_reply(self.locked(
                Method::ShowAlert,
                &[Argument::Text(title), Argument::Text(message)],
            )),
            Request::ShowToast { title, message } => into_reply(self.locked(
                Method::ShowToast,
                &[Argument::Text(title), Argument::Text(message)],
            )),
            Request::ShowTooltip { text } => {
                into_reply(self.locked(Method::ShowTooltip, &[Argument::Text(text)]))
            }
            Request::TriggerHaptic { strength } => {
                succeeded(&self.locked(Method::TriggerHaptic, &[Argument::Text(strength)]))
            }
            Request::WriteClipboard { value } => {
                succeeded(&self.locked(Method::CopyText, &[Argument::Text(value)]))
            }
            Request::ReadClipboard => into_reply(self.locked(Method::PasteText, &[])),
            Request::OpenUrl { url } => {
                succeeded(&self.locked(Method::OpenUrl, &[Argument::Text(url)]))
            }
            Request::Notify {
                delay_ms,
                title,
                payload,
            } => succeeded(&self.locked(
                Method::Notify,
                &[
                    Argument::Long(delay_ms),
                    Argument::Text(title),
                    Argument::Text(payload),
                ],
            )),
            Request::Speak {
                rate_permille,
                text,
                language,
            } => succeeded(&self.locked(
                Method::Speak,
                &[
                    Argument::Long(rate_permille),
                    Argument::Text(text),
                    Argument::Text(language),
                ],
            )),
            Request::PlaySound {
                volume_permille,
                path,
            } => succeeded(&self.locked(
                Method::PlaySound,
                &[
                    Argument::Long(volume_permille),
                    Argument::Text(path),
                    Argument::Text(""),
                ],
            )),
            Request::StopAudio => into_reply(self.locked(Method::StopAudio, &[])),
            Request::WebAuth {
                request,
                url,
                callback_url,
            } => succeeded(&self.locked(
                Method::OpenWebAuth,
                &[
                    Argument::Long(signed(request)),
                    Argument::Text(url),
                    Argument::Text(callback_url),
                ],
            )),
            Request::CancelWebAuth { request } => {
                into_reply(self.locked(Method::CancelWebAuth, &[Argument::Long(signed(request))]))
            }
            Request::Confirm { request, spec } => succeeded(&self.locked(
                Method::ShowConfirm,
                &[Argument::Long(signed(request)), Argument::Text(spec)],
            )),
            Request::OpenSheet { sheet, spec } => succeeded(&self.locked(
                Method::OpenSheet,
                &[Argument::Long(signed(sheet)), Argument::Text(spec)],
            )),
            Request::UpdateSheet { sheet, spec } => into_reply(self.locked(
                Method::UpdateSheet,
                &[Argument::Long(signed(sheet)), Argument::Text(spec)],
            )),
            Request::CloseSheet { sheet } => {
                into_reply(self.locked(Method::CloseSheet, &[Argument::Long(signed(sheet))]))
            }
            Request::PickFile {
                request,
                extensions,
            } => succeeded(&self.locked(
                Method::PickFile,
                &[Argument::Long(signed(request)), Argument::Text(extensions)],
            )),
            Request::CancelFilePick { request } => {
                into_reply(self.locked(Method::CancelFilePick, &[Argument::Long(signed(request))]))
            }
            Request::FetchPickedMedia { limit } => {
                into_reply(self.locked(Method::FetchPickedMedia, &[Argument::Long(signed(limit))]))
            }
            Request::PressNode { identifier, prefix } => into_reply(self.call(
                Method::PressNode,
                &[Argument::Text(identifier), Argument::Flag(prefix)],
            )),
            Request::InsertComposerText { text, replace } => into_reply(self.call(
                Method::InsertComposerText,
                &[Argument::Text(text), Argument::Flag(replace)],
            )),
            Request::DispatchGateway { message } | Request::InjectGateway { message } => {
                succeeded(&self.locked(Method::DispatchGateway, &[Argument::Text(message)]))
            }
            Request::RefreshEmoji | Request::RefreshDecorations => {
                into_reply(self.locked(Method::RefreshEmoji, &[]))
            }
            Request::OpenReport { text, .. } => {
                into_reply(self.locked(Method::OpenReport, &[Argument::Text(text)]))
            }
            Request::ExportBackup => into_reply(self.locked(Method::ExportBackup, &[])),
            Request::ImportBackup => into_reply(self.locked(Method::ImportBackup, &[])),
            Request::RestartGatewaySession => {
                into_reply(self.locked(Method::RestartGatewaySession, &[]))
            }
            Request::EditText {
                request,
                title,
                value,
                maximum,
            } => succeeded(&self.locked(
                Method::EditText,
                &[
                    Argument::Long(signed(request)),
                    Argument::Text(title),
                    Argument::Text(value),
                    Argument::Int(maximum),
                ],
            )),
            Request::EnvironmentFacts => match into_reply(self.call(Method::EnvironmentFacts, &[]))
            {
                Reply::Text(platform) => Reply::Text(Some(crate::facts::merge(
                    platform.as_deref().unwrap_or("{}"),
                ))),
                reply => reply,
            },
            Request::DiscordRequest {
                request,
                method,
                path,
                body,
            } => into_reply(self.locked(
                Method::DiscordRequest,
                &[
                    Argument::Long(signed(request)),
                    Argument::Text(method),
                    Argument::Text(path),
                    Argument::Text(body),
                ],
            )),
            Request::SyncFolderName => into_reply(self.call(Method::SyncFolderName, &[])),
            Request::SyncChooseFolder => into_reply(self.locked(Method::SyncChooseFolder, &[])),
            Request::SyncForgetFolder => into_reply(self.call(Method::SyncForgetFolder, &[])),
            Request::SyncRead => into_reply(self.call(Method::SyncRead, &[])),
            Request::SyncWrite { contents } => {
                into_reply(self.call(Method::SyncWrite, &[Argument::Text(contents)]))
            }
            Request::AppIcons
            | Request::SetAppIcon { .. }
            | Request::KeepAlive { .. }
            | Request::OfferPick { .. }
            | Request::DismissSheets
            | Request::RefreshSettings => Reply::Unsupported,
        }
    }
}

fn signed(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}
