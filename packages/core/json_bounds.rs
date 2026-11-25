use core::ffi::{c_char, c_int};

use crate::bridge::{AstralRustString, bytes};

fn valid_sequence_length(text: &[u8], index: usize) -> usize {
    let lead = text[index];
    let (length, lowest, highest) = match lead {
        0x00..=0x7F => return 1,
        0xC2..=0xDF => (2, 0x80, 0xBF),
        0xE0..=0xEF => (3, if lead == 0xE0 { 0xA0 } else { 0x80 }, if lead == 0xED { 0x9F } else { 0xBF }),
        0xF0..=0xF4 => (4, if lead == 0xF0 { 0x90 } else { 0x80 }, if lead == 0xF4 { 0x8F } else { 0xBF }),
        _ => return 0,
    };
    if index + length > text.len() {
        return 0;
    }
    let continuations_valid = (1..length).all(|offset| {
        let (minimum, maximum) = if offset == 1 { (lowest, highest) } else { (0x80, 0xBF) };
        (minimum..=maximum).contains(&text[index + offset])
    });
    if continuations_valid { length } else { 0 }
}

pub fn with_valid_utf8(text: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(text.len());
    let mut index = 0;
    while index < text.len() {
        let length = valid_sequence_length(text, index);
        if length > 0 {
            result.extend_from_slice(&text[index..index + length]);
            index += length;
        } else {
            result.extend_from_slice("\u{FFFD}".as_bytes());
            index += 1;
        }
    }
    result
}

pub fn nests_too_deeply(text: &[u8], maximum_depth: usize) -> bool {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for &character in text {
        if in_string {
            if escaped {
                escaped = false;
            } else if character == b'\\' {
                escaped = true;
            } else if character == b'"' {
                in_string = false;
            }
            continue;
        }
        match character {
            b'"' => in_string = true,
            b'{' | b'[' => {
                depth += 1;
                if depth > maximum_depth {
                    return true;
                }
            }
            b'}' | b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    false
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_json_valid_utf8(text: *const c_char, length: usize) -> AstralRustString {
    AstralRustString::new(with_valid_utf8(bytes(text, length)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_json_nests_too_deeply(text: *const c_char, length: usize, maximum_depth: usize) -> c_int {
    c_int::from(nests_too_deeply(bytes(text, length), maximum_depth))
}
