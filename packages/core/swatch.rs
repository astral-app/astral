use core::ffi::{c_char, c_int};
use std::collections::HashMap;
use std::sync::Mutex;

use crate::bridge::{AstralRustString, bytes};

const OPAQUE_ALPHA: u32 = 0xFF;

const SWATCH_SIZE: u32 = 48;
const RING_WIDTH: f64 = 2.0;
const RING_COLOR: u32 = 0x5580_8080;
const CACHE_LIMIT: usize = 64;

static CACHE: Mutex<Option<HashMap<u32, Vec<u8>>>> = Mutex::new(None);

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xEDB8_8320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

fn put_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

fn stored_zlib(raw: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let (mut low_sum, mut high_sum) = (1u32, 0u32);
    for byte in raw {
        low_sum = (low_sum + u32::from(*byte)) % 65521;
        high_sum = (high_sum + low_sum) % 65521;
    }
    let mut offset = 0;
    loop {
        let length = (raw.len() - offset).min(65535);
        out.push(u8::from(offset + length >= raw.len()));
        out.extend_from_slice(&(length as u16).to_le_bytes());
        out.extend_from_slice(&(!(length as u16)).to_le_bytes());
        out.extend_from_slice(&raw[offset..offset + length]);
        offset += length;
        if offset >= raw.len() {
            break;
        }
    }
    out.extend_from_slice(&((high_sum << 16) | low_sum).to_be_bytes());
    out
}

fn unpack(argb: u32) -> [f64; 4] {
    [
        f64::from((argb >> 16) & 0xFF),
        f64::from((argb >> 8) & 0xFF),
        f64::from(argb & 0xFF),
        f64::from(argb >> 24) / 255.0,
    ]
}

fn over(top: [f64; 4], coverage: f64, bottom: [f64; 4]) -> [f64; 4] {
    let alpha = top[3] * coverage;
    let out_alpha = alpha + bottom[3] * (1.0 - alpha);
    if out_alpha <= 0.0 {
        return [0.0; 4];
    }
    let mut result = [0.0; 4];
    for channel in 0..3 {
        result[channel] = (top[channel] * alpha + bottom[channel] * bottom[3] * (1.0 - alpha)) / out_alpha;
    }
    result[3] = out_alpha;
    result
}

fn swatch_pixels(argb: u32) -> Vec<u8> {
    let size = SWATCH_SIZE as usize;
    let mut raw = Vec::with_capacity(size * (size * 4 + 1));
    let center = f64::from(SWATCH_SIZE) / 2.0;
    let radius = center - 1.0;
    let (fill, ring) = (unpack(argb), unpack(RING_COLOR));
    for y in 0..SWATCH_SIZE {
        raw.push(0);
        for x in 0..SWATCH_SIZE {
            let distance = (f64::from(x) + 0.5 - center).hypot(f64::from(y) + 0.5 - center);
            let disc = (radius - distance + 0.5).clamp(0.0, 1.0);
            let inner = (radius - RING_WIDTH - distance + 0.5).clamp(0.0, 1.0);
            let pixel = over(fill, inner, over(ring, disc - inner, [0.0; 4]));
            for value in &pixel[..3] {
                raw.push(value.round() as u8);
            }
            raw.push((pixel[3] * 255.0).round() as u8);
        }
    }
    raw
}

fn base64(data: &[u8]) -> Vec<u8> {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let value = (u32::from(chunk[0]) << 16) | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8) | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(ALPHABET[(value >> 18) as usize & 63]);
        out.push(ALPHABET[(value >> 12) as usize & 63]);
        out.push(if chunk.len() > 1 {
            ALPHABET[(value >> 6) as usize & 63]
        } else {
            b'='
        });
        out.push(if chunk.len() > 2 { ALPHABET[value as usize & 63] } else { b'=' });
    }
    out
}

fn encode_swatch(argb: u32) -> Vec<u8> {
    let mut png = vec![0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n'];
    let mut header = Vec::new();
    header.extend_from_slice(&SWATCH_SIZE.to_be_bytes());
    header.extend_from_slice(&SWATCH_SIZE.to_be_bytes());
    header.extend_from_slice(&[8, 6, 0, 0, 0]);
    put_chunk(&mut png, b"IHDR", &header);
    put_chunk(&mut png, b"IDAT", &stored_zlib(&swatch_pixels(argb)));
    put_chunk(&mut png, b"IEND", &[]);
    let mut uri = b"data:image/png;base64,".to_vec();
    uri.extend_from_slice(&base64(&png));
    uri
}

pub fn parse_color(text: &[u8]) -> Option<u32> {
    if (text.len() != 7 && text.len() != 9) || text[0] != b'#' {
        return None;
    }
    let value = u32::from_str_radix(std::str::from_utf8(&text[1..]).ok()?, 16).ok()?;
    if !text[1..].iter().all(u8::is_ascii_hexdigit) {
        return None;
    }
    Some(if text.len() == 7 { 0xFF00_0000 | value } else { value })
}

pub fn color_text(argb: u32) -> String {
    if argb >> 24 == OPAQUE_ALPHA {
        format!("#{:06X}", argb & 0xFF_FFFF)
    } else {
        format!("#{argb:08X}")
    }
}

pub fn swatch_uri(argb: u32) -> Vec<u8> {
    let mut guard = CACHE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let cache = guard.get_or_insert_with(HashMap::new);
    if let Some(found) = cache.get(&argb) {
        return found.clone();
    }
    if cache.len() > CACHE_LIMIT {
        cache.clear();
    }
    let uri = encode_swatch(argb);
    cache.insert(argb, uri.clone());
    uri
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_parse_color(text: *const c_char, length: usize, color: *mut u32) -> c_int {
    let Some(parsed) = parse_color(bytes(text, length)) else { return 0 };
    if let Some(color) = unsafe { color.as_mut() } {
        *color = parsed;
    }
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_color_text(argb: u32) -> AstralRustString {
    AstralRustString::new(color_text(argb).into_bytes())
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_swatch_uri(argb: u32) -> AstralRustString {
    AstralRustString::new(swatch_uri(argb))
}
