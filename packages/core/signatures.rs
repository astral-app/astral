use core::ffi::{c_char, c_int, c_void};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use ed25519_dalek::{Signature, VerifyingKey};

use crate::bridge::{AstralRustString, bytes, path};
use crate::files::resolve_inside;

const SHA256_HEX_LENGTH: usize = 64;

const SIGNATURE_LIMIT: usize = 128 << 10;
const FILE_LIMIT: usize = 1 << 20;
const TOTAL_LIMIT: usize = 2 << 20;
const MAX_FILES: usize = 512;
const MAX_ENTRIES: usize = 2048;
const MESSAGE_HEADER: &str = "astral-plugin-v2\n";

pub type Digest = unsafe extern "C" fn(*mut c_void, *const c_char, usize, *mut AstralRustString);

fn read_regular(file: &Path, maximum: usize) -> Option<Vec<u8>> {
    let mut handle = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(file)
        .ok()?;
    let metadata = handle.metadata().ok()?;
    if !metadata.file_type().is_file() || metadata.len() > maximum as u64 {
        return None;
    }
    let mut contents = Vec::new();
    handle.by_ref().take(maximum as u64 + 1).read_to_end(&mut contents).ok()?;
    (contents.len() <= maximum).then_some(contents)
}

fn base64(text: &[u8]) -> Option<Vec<u8>> {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let text = text.trim_ascii();
    if text.is_empty() || !text.len().is_multiple_of(4) || text.len() > 128 {
        return None;
    }
    let mut decoded = Vec::new();
    for (index, chunk) in text.chunks(4).enumerate() {
        let last = (index + 1) * 4 == text.len();
        let mut value = 0u32;
        let mut padding = 0;
        for (digit, character) in chunk.iter().enumerate() {
            if *character == b'=' {
                if !last || digit < 2 {
                    return None;
                }
                padding += 1;
                value <<= 6;
            } else {
                let position = ALPHABET.iter().position(|letter| letter == character)?;
                if padding > 0 {
                    return None;
                }
                value = (value << 6) | position as u32;
            }
        }
        if padding > 2 || (padding == 2 && value & 0xFFFF != 0) || (padding == 1 && value & 0xFF != 0) {
            return None;
        }
        decoded.push((value >> 16) as u8);
        if padding < 2 {
            decoded.push((value >> 8) as u8);
        }
        if padding < 1 {
            decoded.push(value as u8);
        }
    }
    Some(decoded)
}

fn safe_path(relative: &str) -> bool {
    !relative.is_empty()
        && relative.len() <= 240
        && !relative.contains(['\\', '\0'])
        && relative.split('/').all(|part| !part.is_empty() && part != "." && part != "..")
}

pub fn verify(public_key: &[u8], message: &[u8], signature: &[u8]) -> bool {
    let (Some(key), Some(signature)) = (base64(public_key), base64(signature)) else {
        return false;
    };
    let (Ok(key), Ok(signature)) = (<[u8; 32]>::try_from(key), <[u8; 64]>::try_from(signature)) else {
        return false;
    };
    let Ok(key) = VerifyingKey::from_bytes(&key) else { return false };
    key.verify_strict(message, &Signature::from_bytes(&signature)).is_ok()
}

pub fn signed_files(public_key: &[u8], document: &[u8]) -> Option<BTreeMap<String, String>> {
    if document.len() > SIGNATURE_LIMIT {
        return None;
    }
    let parsed = crate::toml_json::parse(std::str::from_utf8(document).ok()?)?;
    let entries = parsed.get("files")?.as_object()?;
    let signature = parsed.get("signature")?.as_str()?;
    if entries.is_empty() || entries.len() > MAX_FILES {
        return None;
    }
    let mut files = BTreeMap::new();
    for (relative, value) in entries {
        let digest = value.as_str()?;
        if !safe_path(relative)
            || digest.len() != SHA256_HEX_LENGTH
            || !digest.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        {
            return None;
        }
        files.insert(relative.clone(), digest.to_owned());
    }
    let mut message = MESSAGE_HEADER.to_owned();
    for (relative, digest) in &files {
        message.push_str(relative);
        message.push(' ');
        message.push_str(digest);
        message.push('\n');
    }
    verify(public_key, message.as_bytes(), signature.as_bytes()).then_some(files)
}

fn digest_of(digest: Digest, context: *mut c_void, contents: &[u8]) -> Vec<u8> {
    let mut result = AstralRustString {
        bytes: core::ptr::null_mut(),
        length: 0,
    };
    unsafe { digest(context, contents.as_ptr().cast(), contents.len(), &mut result) };
    let value = bytes(result.bytes.cast(), result.length).to_vec();
    unsafe { crate::bridge::astral_rust_string_free(result) };
    value
}

fn unsigned_code(root: &Path, files: &BTreeMap<String, String>) -> Option<bool> {
    let mut pending = vec![root.to_path_buf()];
    let mut entries = 0;
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).ok()? {
            let entry = entry.ok()?;
            entries += 1;
            let kind = entry.file_type().ok()?;
            if entries > MAX_ENTRIES || kind.is_symlink() {
                return None;
            }
            let full = entry.path();
            let relative = full.strip_prefix(root).ok()?.to_str()?.to_owned();
            if relative == "tests" {
                continue;
            }
            if kind.is_dir() {
                pending.push(full);
                continue;
            }
            if !kind.is_file() {
                continue;
            }
            let lua = relative.ends_with(".lua") || relative.ends_with(".luac");
            let translation = relative.split('/').next() == Some("i18n");
            if (lua || translation) && !files.contains_key(&relative) {
                return Some(true);
            }
        }
    }
    Some(false)
}

pub fn verify_directory(public_key: &[u8], directory: &[u8], main: &str, digest: impl Fn(&[u8]) -> Vec<u8>) -> bool {
    if !safe_path(main) {
        return false;
    }
    let Some(signature) = resolve_inside(directory, b"signature") else {
        return false;
    };
    let Some(document) = read_regular(path(signature.as_ptr().cast(), signature.len()), SIGNATURE_LIMIT) else {
        return false;
    };
    let Some(files) = signed_files(public_key, &document) else {
        return false;
    };
    if !files.contains_key("manifest.toml") || !files.contains_key(main) {
        return false;
    }
    let mut total = 0;
    for (relative, expected) in &files {
        let Some(file) = resolve_inside(directory, relative.as_bytes()) else {
            return false;
        };
        let Some(contents) = read_regular(path(file.as_ptr().cast(), file.len()), FILE_LIMIT) else {
            return false;
        };
        total += contents.len();
        if total > TOTAL_LIMIT || digest(&contents) != expected.as_bytes() {
            return false;
        }
    }
    unsigned_code(path(directory.as_ptr().cast(), directory.len()), &files) == Some(false)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_signatures_verify(
    key: *const c_char,
    key_length: usize,
    message: *const c_char,
    message_length: usize,
    signature: *const c_char,
    signature_length: usize,
) -> c_int {
    c_int::from(verify(
        bytes(key, key_length),
        bytes(message, message_length),
        bytes(signature, signature_length),
    ))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_signatures_files(
    key: *const c_char,
    key_length: usize,
    document: *const c_char,
    document_length: usize,
    into: *mut AstralRustString,
) -> c_int {
    let Some(files) = signed_files(bytes(key, key_length), bytes(document, document_length)) else {
        return 0;
    };
    let flat: Vec<Vec<u8>> = files
        .into_iter()
        .flat_map(|(relative, digest)| [relative.into_bytes(), digest.into_bytes()])
        .collect();
    if let Some(into) = unsafe { into.as_mut() } {
        *into = AstralRustString::list(&flat);
    }
    1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_signatures_directory(
    key: *const c_char,
    key_length: usize,
    directory: *const c_char,
    directory_length: usize,
    main: *const c_char,
    main_length: usize,
    digest: Option<Digest>,
    context: *mut c_void,
) -> c_int {
    let Some(digest) = digest else { return 0 };
    let Ok(main) = std::str::from_utf8(bytes(main, main_length)) else {
        return 0;
    };
    c_int::from(verify_directory(
        bytes(key, key_length),
        bytes(directory, directory_length),
        main,
        |contents| digest_of(digest, context, contents),
    ))
}
