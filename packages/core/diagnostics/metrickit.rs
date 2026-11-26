use core::ffi::{c_char, c_int};
use std::sync::RwLock;

use serde_json::Value;

use crate::support::bridge::{AstralRustString, bytes};

const MAX_FRAMES: usize = 60;

pub const CRASH: c_int = 0;
pub const HANG: c_int = 1;
pub const DISK_WRITE: c_int = 2;

pub type ImageCheck = unsafe extern "C" fn(*const c_char, usize) -> c_int;

static IMAGE_CHECK: RwLock<Option<ImageCheck>> = RwLock::new(None);
static APP_IMAGES: RwLock<Option<AppImages>> = RwLock::new(None);

pub struct AppImages {
    pub loaded: std::collections::BTreeSet<String>,
    pub executable: String,
    pub frameworks: String,
}

impl AppImages {
    fn contains(&self, binary: &str) -> bool {
        let frameworks = std::path::Path::new(&self.frameworks);
        self.loaded.contains(binary)
            || binary == self.executable
            || frameworks.join(format!("{binary}.framework")).exists()
            || frameworks.join(binary).exists()
    }
}

pub fn set_app_images(images: AppImages) {
    *APP_IMAGES.write().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(images);
}

fn unsigned_member(object: &Value, key: &str) -> u64 {
    match object.get(key) {
        Some(value) => value.as_u64().or_else(|| value.as_i64().map(|signed| signed as u64)).unwrap_or(0),
        None => 0,
    }
}

fn text_member(object: &Value, key: &str) -> String {
    match object.get(key) {
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

fn crashing_thread(diagnostic: &Value) -> Option<&Value> {
    let stacks = diagnostic.get("callStackTree")?.get("callStacks")?.as_array()?;
    stacks
        .iter()
        .find(|stack| stack.get("threadAttributed").and_then(Value::as_bool) == Some(true))
        .or_else(|| stacks.first())
}

fn from_this_app(frame: &Value) -> bool {
    let binary = text_member(frame, "binaryName");
    if binary.is_empty() {
        return true;
    }
    if let Some(images) = APP_IMAGES.read().unwrap_or_else(|poisoned| poisoned.into_inner()).as_ref() {
        return images.contains(&binary);
    }
    let Some(check) = *IMAGE_CHECK.read().unwrap_or_else(|poisoned| poisoned.into_inner()) else {
        return true;
    };
    unsafe { check(binary.as_ptr().cast(), binary.len()) != 0 }
}

fn frame_line(index: usize, frame: &Value) -> String {
    let mut line = format!("#{index} {:#x}", unsigned_member(frame, "address"));
    let binary = text_member(frame, "binaryName");
    if !binary.is_empty() {
        line.push_str(&format!(" {binary} + {:#x}", unsigned_member(frame, "offsetIntoBinaryTextSegment")));
    }
    line.push('\n');
    line
}

fn attributed_frames(diagnostic: &Value) -> Option<String> {
    let thread = diagnostic.is_object().then(|| crashing_thread(diagnostic)).flatten()?;
    let mut frame = thread.get("callStackRootFrames")?.as_array()?.first();
    let mut text = String::new();
    let mut index = 0;
    while let Some(current) = frame.filter(|current| current.is_object() && index < MAX_FRAMES) {
        if !from_this_app(current) {
            return None;
        }
        text.push_str(&frame_line(index, current));
        frame = current
            .get("subFrames")
            .and_then(Value::as_array)
            .and_then(|children| children.first());
        index += 1;
    }
    Some(text)
}

fn metadata(diagnostic: &Value) -> Value {
    diagnostic
        .get("diagnosticMetaData")
        .cloned()
        .unwrap_or_else(|| Value::Object(Default::default()))
}

pub fn crash_stack(crash: &Value) -> Option<String> {
    let frames = attributed_frames(crash)?;
    let metadata = metadata(crash);
    let mut text = format!("Signal {}\n", text_member(&metadata, "signal"));
    let exception = text_member(&metadata, "exceptionType");
    if !exception.is_empty() {
        text.push_str(&format!("Exception {exception} code {}\n", text_member(&metadata, "exceptionCode")));
    }
    let reason = text_member(&metadata, "terminationReason");
    if !reason.is_empty() {
        text.push_str(&format!("Termination {reason}\n"));
    }
    Some(text + &frames)
}

pub fn hang_stack(hang: &Value) -> Option<String> {
    let frames = attributed_frames(hang)?;
    Some(format!("Hang {}\n{frames}", text_member(&metadata(hang), "hangDuration")))
}

pub fn disk_write_stack(disk_write: &Value) -> Option<String> {
    let frames = attributed_frames(disk_write)?;
    Some(format!(
        "Disk writes {}\n{frames}",
        text_member(&metadata(disk_write), "writesCaused")
    ))
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_metrickit_set_image_check(check: Option<ImageCheck>) {
    *IMAGE_CHECK.write().unwrap_or_else(|poisoned| poisoned.into_inner()) = check;
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_metrickit_stack(kind: c_int, json: *const c_char, length: usize) -> AstralRustString {
    let none = AstralRustString {
        bytes: core::ptr::null_mut(),
        length: 0,
    };
    let Ok(diagnostic) = serde_json::from_slice::<Value>(bytes(json, length)) else {
        return none;
    };
    let stack = match kind {
        CRASH => crash_stack(&diagnostic),
        HANG => hang_stack(&diagnostic),
        DISK_WRITE => disk_write_stack(&diagnostic),
        _ => None,
    };
    stack.map_or(none, |text| AstralRustString::new(text.into_bytes()))
}
