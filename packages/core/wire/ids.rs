use core::ffi::{CStr, c_char, c_int};
use std::sync::atomic::{AtomicU64, Ordering};

const MIN_SNOWFLAKE_DIGITS: usize = 15;
const AVATAR_MARKERS: [&[u8]; 2] = [b"/avatars/", b"/users/"];

static CURRENT_CHANNEL: AtomicU64 = AtomicU64::new(0);

pub fn parse_id(text: &[u8]) -> u64 {
    let digits = text.iter().take_while(|byte| byte.is_ascii_digit()).count();
    if digits < MIN_SNOWFLAKE_DIGITS {
        return 0;
    }
    text[..digits]
        .iter()
        .fold(0u64, |value, digit| value.wrapping_mul(10).wrapping_add(u64::from(digit - b'0')))
}

pub fn avatar_user(text: &[u8]) -> u64 {
    for marker in AVATAR_MARKERS {
        for index in 0..text.len().saturating_sub(marker.len()) {
            if text[index..].starts_with(marker) {
                let user = parse_id(&text[index + marker.len()..]);
                if user != 0 {
                    return user;
                }
            }
        }
    }
    0
}

pub fn prefixed(text: &[u8], prefix: &[u8]) -> Option<u64> {
    if text.len() <= prefix.len() || !text.starts_with(prefix) {
        return None;
    }
    let id = parse_id(&text[prefix.len()..]);
    (id != 0).then_some(id)
}

pub fn current_channel() -> u64 {
    CURRENT_CHANNEL.load(Ordering::Relaxed)
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_wire_current_channel() -> u64 {
    current_channel()
}

#[unsafe(export_name = "wireSetCurrentChannel")]
pub extern "C" fn wire_set_current_channel(channel: u64) {
    CURRENT_CHANNEL.store(channel, Ordering::Relaxed);
}

#[unsafe(export_name = "wireAvatarUser")]
pub unsafe extern "C" fn wire_avatar_user(text: *const u8, length: usize) -> u64 {
    if text.is_null() {
        return 0;
    }
    avatar_user(unsafe { core::slice::from_raw_parts(text, length) })
}

#[unsafe(export_name = "wirePrefixed")]
pub unsafe extern "C" fn wire_prefixed(text: *const u8, length: usize, prefix: *const c_char, suffix: *mut u64) -> c_int {
    if text.is_null() || prefix.is_null() || suffix.is_null() {
        return 0;
    }
    let text = unsafe { core::slice::from_raw_parts(text, length) };
    let id = prefixed(text, unsafe { CStr::from_ptr(prefix) }.to_bytes()).unwrap_or(0);
    unsafe { *suffix = id };
    c_int::from(id != 0)
}
