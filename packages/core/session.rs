use core::ffi::{c_char, c_int};
use std::sync::Mutex;

use crate::bridge::{AstralRustString, bytes};

const MIN_IDENTITY: usize = 20;
const MIN_TIMESTAMP: usize = 4;
const MIN_SIGNATURE: usize = 20;

static TOKEN: Mutex<Vec<u8>> = Mutex::new(Vec::new());

fn token_character(character: u8) -> bool {
    character.is_ascii_alphanumeric() || character == b'_' || character == b'-'
}

pub fn plausible(token: &[u8]) -> bool {
    let parts: Vec<&[u8]> = token.split(|byte| *byte == b'.').collect();
    let [identity, timestamp, signature] = parts.as_slice() else {
        return false;
    };
    if identity.len() < MIN_IDENTITY || timestamp.len() < MIN_TIMESTAMP || signature.len() < MIN_SIGNATURE {
        return false;
    }
    token.iter().all(|character| *character == b'.' || token_character(*character))
}

pub fn current() -> Vec<u8> {
    TOKEN.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_session_plausible_token(token: *const c_char, length: usize) -> c_int {
    c_int::from(plausible(bytes(token, length)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_session_set_token(token: *const c_char, length: usize) {
    let token = bytes(token, length);
    if plausible(token) {
        *TOKEN.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = token.to_vec();
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_session_token() -> AstralRustString {
    AstralRustString::new(current())
}
