mod apk_signer;
mod bridge;
mod facts;

use std::sync::{Arc, OnceLock};

use astral_core::app::state::Platform;
use astral_core::app::{self, CompletedRequest, Launch};
use jni::JNIEnv;
use jni::JavaVM;
use jni::objects::{JByteArray, JClass, JObjectArray, JString};
use jni::sys::{
    JNI_FALSE, JNI_TRUE, JNI_VERSION_1_6, jboolean, jbyteArray, jdouble, jint, jlong, jstring,
};

use self::bridge::JniBridge;

const MAX_PROPERTY_NAMES: i32 = 2048;
const FILE_PICK_LIMIT: usize = 1 << 20;
const FILE_TOO_LARGE: &str = "file exceeds 1 MiB";
const FILE_UNREADABLE: &str = "file could not be read";

static VM: OnceLock<JavaVM> = OnceLock::new();

fn text(env: &mut JNIEnv, value: &JString) -> String {
    if value.is_null() {
        return String::new();
    }
    env.get_string(value).map(String::from).unwrap_or_default()
}

fn optional_text(env: &mut JNIEnv, value: &JString) -> Option<String> {
    (!value.is_null()).then(|| text(env, value))
}

fn texts(env: &mut JNIEnv, array: &JObjectArray) -> Vec<String> {
    if array.is_null() {
        return Vec::new();
    }
    let count = env
        .get_array_length(array)
        .unwrap_or(0)
        .min(MAX_PROPERTY_NAMES);
    let mut values = Vec::new();
    for index in 0..count {
        let Ok(element) = env.get_object_array_element(array, index) else {
            continue;
        };
        let value = JString::from(element);
        values.push(text(env, &value));
        let _ = env.delete_local_ref(value);
    }
    values
}

fn bytes(env: &mut JNIEnv, array: &JByteArray) -> Vec<u8> {
    if array.is_null() {
        return Vec::new();
    }
    env.convert_byte_array(array).unwrap_or_default()
}

fn java_text(env: &mut JNIEnv, value: &str) -> jstring {
    env.new_string(value)
        .map_or(core::ptr::null_mut(), JString::into_raw)
}

fn optional_java_text(env: &mut JNIEnv, value: Option<String>) -> jstring {
    value.map_or(core::ptr::null_mut(), |value| java_text(env, &value))
}

fn java_bytes(env: &mut JNIEnv, value: Option<Vec<u8>>) -> jbyteArray {
    value
        .filter(|value| !value.is_empty())
        .and_then(|value| env.byte_array_from_slice(&value).ok())
        .map_or(core::ptr::null_mut(), JByteArray::into_raw)
}

const fn flag(value: bool) -> jboolean {
    if value { JNI_TRUE } else { JNI_FALSE }
}

fn unsigned(value: jlong) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "system" fn JNI_OnLoad(vm: JavaVM, _: *mut core::ffi::c_void) -> jint {
    let _ = VM.set(vm);
    JNI_VERSION_1_6
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_astral_android_Native_initialize<'local>(
    mut env: JNIEnv<'local>,
    _: JClass<'local>,
    names: JObjectArray<'local>,
    data_directory: JString<'local>,
    resource_directory: JString<'local>,
    languages: JObjectArray<'local>,
    discord_version: JString<'local>,
    environment: JString<'local>,
) -> jint {
    let launch = Launch {
        platform: Platform::Android,
        version: env!("ASTRAL_VERSION").to_owned(),
        commit: env!("ASTRAL_COMMIT").to_owned(),
        property_names: texts(&mut env, &names),
        state_directory: format!("{}/files/astral", text(&mut env, &data_directory)),
        discord_directory: format!("{}/files", text(&mut env, &data_directory)),
        resource_directory: text(&mut env, &resource_directory),
        languages: texts(&mut env, &languages),
        discord_version: text(&mut env, &discord_version),
        environment: serde_json::from_str(&text(&mut env, &environment))
            .unwrap_or(serde_json::Value::Null),
    };
    jint::try_from(app::initialize_in_background(launch)).unwrap_or(jint::MAX)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_astral_android_Native_attachBridge<'local>(
    mut env: JNIEnv<'local>,
    _: JClass<'local>,
    class: JClass<'local>,
) {
    let Some(vm) = env.get_java_vm().ok() else {
        return;
    };
    let Some(methods) = JniBridge::resolve(&mut env, &class) else {
        return;
    };
    let Ok(reference) = env.new_global_ref(&class) else {
        return;
    };
    app::set_bridge(Arc::new(JniBridge::new(vm, reference, methods)));
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_astral_android_Native_markVisible<'local>(
    _: JNIEnv<'local>,
    _: JClass<'local>,
) {
    app::mark_visible();
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_astral_android_Native_markHidden<'local>(
    _: JNIEnv<'local>,
    _: JClass<'local>,
) {
    app::mark_hidden();
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_astral_android_Native_markReady<'local>(
    _: JNIEnv<'local>,
    _: JClass<'local>,
) {
    app::mark_ready();
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_astral_android_Native_renderSvg<'local>(
    mut env: JNIEnv<'local>,
    _: JClass<'local>,
    svg: JByteArray<'local>,
    width: jint,
    height: jint,
) -> jbyteArray {
    let svg = bytes(&mut env, &svg);
    let size = |value: jint| u32::try_from(value).unwrap_or(0);
    let png = astral_core::render::svg::png(&svg, size(width), size(height));
    java_bytes(&mut env, png)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_astral_android_Native_rewrite<'local>(
    mut env: JNIEnv<'local>,
    _: JClass<'local>,
    ops: JByteArray<'local>,
    length: jint,
) -> jbyteArray {
    let all = bytes(&mut env, &ops);
    let Some(length) = usize::try_from(length)
        .ok()
        .filter(|length| *length > 0 && *length <= all.len())
    else {
        return core::ptr::null_mut();
    };
    let rewritten = app::rewrite(&all[..length]);
    java_bytes(&mut env, rewritten)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_astral_android_Native_plugins<'local>(
    mut env: JNIEnv<'local>,
    _: JClass<'local>,
) -> jstring {
    java_text(&mut env, &app::plugins())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_astral_android_Native_setEnabled<'local>(
    mut env: JNIEnv<'local>,
    _: JClass<'local>,
    identifier: JString<'local>,
    enabled: jboolean,
) {
    app::set_enabled(&text(&mut env, &identifier), enabled == JNI_TRUE);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_astral_android_Native_eval<'local>(
    mut env: JNIEnv<'local>,
    _: JClass<'local>,
    identifier: JString<'local>,
    code: JString<'local>,
) -> jstring {
    let result = app::eval(&text(&mut env, &identifier), &text(&mut env, &code));
    java_text(&mut env, &result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_astral_android_Native_composerEvent<'local>(
    mut env: JNIEnv<'local>,
    _: JClass<'local>,
    event: JByteArray<'local>,
) -> jbyteArray {
    let event = bytes(&mut env, &event);
    let before = app::composer_event(&event);
    java_bytes(&mut env, before)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_astral_android_Native_pendingUpdates<'local>(
    mut env: JNIEnv<'local>,
    _: JClass<'local>,
) -> jbyteArray {
    let updates = app::pending_updates();
    java_bytes(&mut env, Some(updates))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_astral_android_Native_tweensActive<'local>(
    _: JNIEnv<'local>,
    _: JClass<'local>,
) -> jboolean {
    flag(app::tweens_active())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_astral_android_Native_tweenFrame<'local>(
    mut env: JNIEnv<'local>,
    _: JClass<'local>,
    now_ms: jdouble,
) -> jbyteArray {
    let frame = app::tween_frame(now_ms);
    java_bytes(&mut env, Some(frame))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_astral_android_Native_back<'local>(
    _: JNIEnv<'local>,
    _: JClass<'local>,
) -> jboolean {
    flag(app::back())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_astral_android_Native_refreshDecorations<'local>(
    _: JNIEnv<'local>,
    _: JClass<'local>,
) {
    app::refresh_decorations();
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_astral_android_Native_pageOpen<'local>(
    _: JNIEnv<'local>,
    _: JClass<'local>,
) -> jboolean {
    flag(app::page_open())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_astral_android_Native_completeRequest<'local>(
    mut env: JNIEnv<'local>,
    _: JClass<'local>,
    request: jlong,
    status: jint,
    ok: jboolean,
    url: JString<'local>,
    headers: JString<'local>,
    body: JString<'local>,
    body_base64: JString<'local>,
    error: JString<'local>,
) {
    app::complete_request(CompletedRequest {
        request: unsigned(request),
        status: i64::from(status),
        ok: ok == JNI_TRUE,
        url: text(&mut env, &url),
        headers: text(&mut env, &headers),
        body: text(&mut env, &body),
        body_base64: optional_text(&mut env, &body_base64),
        error: optional_text(&mut env, &error),
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_astral_android_Native_sheetEvent<'local>(
    mut env: JNIEnv<'local>,
    _: JClass<'local>,
    sheet: jlong,
    action: JString<'local>,
    value: JString<'local>,
    closed: jboolean,
) {
    app::sheet_event(
        unsigned(sheet),
        text(&mut env, &action),
        text(&mut env, &value),
        closed == JNI_TRUE,
    );
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_astral_android_Native_completeConfirm<'local>(
    _: JNIEnv<'local>,
    _: JClass<'local>,
    request: jlong,
    accepted: jboolean,
) {
    app::complete_confirm(unsigned(request), accepted == JNI_TRUE);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_astral_android_Native_completeWebAuth<'local>(
    mut env: JNIEnv<'local>,
    _: JClass<'local>,
    request: jlong,
    url: JString<'local>,
    cookies: JString<'local>,
    error: JString<'local>,
) {
    app::complete_web_auth(
        unsigned(request),
        optional_text(&mut env, &url),
        optional_text(&mut env, &cookies),
        optional_text(&mut env, &error),
    );
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_astral_android_Native_completeFilePick<'local>(
    mut env: JNIEnv<'local>,
    _: JClass<'local>,
    request: jlong,
    name: JString<'local>,
    data: JByteArray<'local>,
    error: JString<'local>,
) {
    let file_name = optional_text(&mut env, &name);
    let mut failure = optional_text(&mut env, &error);
    let mut contents = None;
    if !data.is_null() {
        let length = env
            .get_array_length(&data)
            .ok()
            .and_then(|length| usize::try_from(length).ok())
            .unwrap_or(0);
        if length > FILE_PICK_LIMIT {
            failure.get_or_insert_with(|| FILE_TOO_LARGE.to_owned());
        } else {
            match env.convert_byte_array(&data) {
                Ok(bytes) => contents = Some(bytes),
                Err(_) => {
                    let _ = env.exception_clear();
                    failure.get_or_insert_with(|| FILE_UNREADABLE.to_owned());
                }
            }
        }
    }
    app::complete_file_pick(
        unsigned(request),
        file_name.as_deref().map(str::as_bytes),
        contents.as_deref(),
        failure.as_deref(),
    );
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_astral_android_Native_completeTextEdit<'local>(
    mut env: JNIEnv<'local>,
    _: JClass<'local>,
    request: jlong,
    value: JString<'local>,
) {
    app::complete_text_edit(unsigned(request), optional_text(&mut env, &value));
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_astral_android_Native_emitEvent<'local>(
    mut env: JNIEnv<'local>,
    _: JClass<'local>,
    event: JString<'local>,
) {
    app::emit_event(&text(&mut env, &event));
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_astral_android_Native_recordExitReport<'local>(
    mut env: JNIEnv<'local>,
    _: JClass<'local>,
    kind: JString<'local>,
    timestamp: jlong,
    reason: jint,
    description: JString<'local>,
    trace: JString<'local>,
) {
    app::record_exit_report(
        &text(&mut env, &kind),
        timestamp,
        i64::from(reason),
        &text(&mut env, &description),
        &text(&mut env, &trace),
    );
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_astral_android_Native_exportBackup<'local>(
    mut env: JNIEnv<'local>,
    _: JClass<'local>,
) -> jstring {
    java_text(&mut env, &app::export_backup())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_astral_android_Native_applyBackup<'local>(
    mut env: JNIEnv<'local>,
    _: JClass<'local>,
    contents: JString<'local>,
) -> jstring {
    let failure = app::apply_backup(&text(&mut env, &contents));
    optional_java_text(&mut env, failure)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_astral_android_Native_hostRequestWanted<'local>(
    mut env: JNIEnv<'local>,
    _: JClass<'local>,
    frame: JByteArray<'local>,
) -> jboolean {
    let frame = bytes(&mut env, &frame);
    flag(app::host_request_wanted(&frame))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_astral_android_Native_interceptHostRequest<'local>(
    mut env: JNIEnv<'local>,
    _: JClass<'local>,
    frame: JByteArray<'local>,
) -> jbyteArray {
    let frame = bytes(&mut env, &frame);
    let rewritten = app::intercept_host_request(&frame);
    java_bytes(&mut env, rewritten)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_astral_android_Native_rewriteHostCall<'local>(
    mut env: JNIEnv<'local>,
    _: JClass<'local>,
    frame: JByteArray<'local>,
) -> jbyteArray {
    let frame = bytes(&mut env, &frame);
    let rewritten = app::rewrite_host_call(&frame);
    java_bytes(&mut env, rewritten)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_astral_android_Native_needsGatewayEvents<'local>(
    _: JNIEnv<'local>,
    _: JClass<'local>,
) -> jboolean {
    flag(app::needs_gateway_events())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_astral_android_Native_processGatewayMessage<'local>(
    mut env: JNIEnv<'local>,
    _: JClass<'local>,
    message: JString<'local>,
) -> jstring {
    let processed = app::process_gateway_message(&text(&mut env, &message));
    optional_java_text(&mut env, processed)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_astral_android_Native_staffDevToolsEnabled<'local>(
    _: JNIEnv<'local>,
    _: JClass<'local>,
) -> jboolean {
    flag(app::staff_dev_tools_enabled())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_astral_android_Native_withStaffReady<'local>(
    mut env: JNIEnv<'local>,
    _: JClass<'local>,
    message: JString<'local>,
) -> jstring {
    let transformed = app::with_staff_ready(&text(&mut env, &message));
    optional_java_text(&mut env, transformed)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_astral_android_Native_noteBlockedRequest<'local>(
    _: JNIEnv<'local>,
    _: JClass<'local>,
) {
    app::note_blocked_request();
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_astral_android_Native_sinkUrl<'local>(
    mut env: JNIEnv<'local>,
    _: JClass<'local>,
) -> jstring {
    java_text(&mut env, &app::sink_url())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_astral_android_Native_anchoredClasses<'local>(
    mut env: JNIEnv<'local>,
    _: JClass<'local>,
    apk: JString<'local>,
    anchors: JString<'local>,
) -> jstring {
    let apk = text(&mut env, &apk);
    let anchors: Vec<String> = serde_json::from_str(&text(&mut env, &anchors)).unwrap_or_default();
    let anchors: Vec<&str> = anchors.iter().map(String::as_str).collect();
    let found = astral_core::support::dex::anchored_classes(&apk, &anchors);
    java_text(&mut env, &found.to_string())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_astral_android_Native_iconPath<'local>(
    mut env: JNIEnv<'local>,
    _: JClass<'local>,
    name: JString<'local>,
) -> jstring {
    let path = app::icon_path(&text(&mut env, &name));
    java_text(&mut env, &path)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_astral_android_Native_emojiURL<'local>(
    mut env: JNIEnv<'local>,
    _: JClass<'local>,
    codepoints: JString<'local>,
) -> jstring {
    let url = if codepoints.is_null() {
        String::new()
    } else {
        app::emoji_url(&text(&mut env, &codepoints))
    };
    java_text(&mut env, &url)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_astral_android_Native_composerStyles<'local>(
    mut env: JNIEnv<'local>,
    _: JClass<'local>,
    value: JString<'local>,
) -> jstring {
    let styles = if value.is_null() {
        "[]".to_owned()
    } else {
        app::composer_styles(&text(&mut env, &value))
    };
    java_text(&mut env, &styles)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_astral_android_Native_networkPlan<'local>(
    mut env: JNIEnv<'local>,
    _: JClass<'local>,
    method: JString<'local>,
    url: JString<'local>,
) -> jni::sys::jint {
    let flags = app::exchange::plan_flags(&text(&mut env, &method), &text(&mut env, &url), false);
    jni::sys::jint::try_from(flags).unwrap_or_default()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_astral_android_Native_networkBegin<'local>(
    mut env: JNIEnv<'local>,
    _: JClass<'local>,
    request: JString<'local>,
    stages: jni::sys::jint,
) -> jstring {
    let stages = u32::try_from(stages).unwrap_or_default();
    let decision = app::exchange::begin(&text(&mut env, &request), stages);
    java_text(&mut env, &decision.to_string())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_astral_android_Native_networkFinish<'local>(
    mut env: JNIEnv<'local>,
    _: JClass<'local>,
    exchange: jni::sys::jlong,
    response: JString<'local>,
) -> jstring {
    let exchange = u64::try_from(exchange).unwrap_or_default();
    let changed =
        app::exchange::finish(exchange, &text(&mut env, &response)).map(|value| value.to_string());
    optional_java_text(&mut env, changed)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_astral_android_Native_networkAbandon<'local>(
    _: JNIEnv<'local>,
    _: JClass<'local>,
    exchange: jni::sys::jlong,
) {
    app::exchange::abandon(u64::try_from(exchange).unwrap_or_default());
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_astral_android_Native_guestEvent<'local>(
    env: JNIEnv<'local>,
    _: JClass<'local>,
    event: JByteArray<'local>,
) -> jni::sys::jint {
    let bytes = env.convert_byte_array(&event).unwrap_or_default();
    app::guest_event::handle(&bytes)
}
