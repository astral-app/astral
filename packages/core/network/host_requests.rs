use core::ffi::c_char;

use crate::support::bridge::{AstralRustString, bytes};
use crate::wire::cursor::Cursor;

use crate::wire::tag::Tag;

const HEADER_PAIR: u32 = 2;

const HEADER: usize = 16;
const CALL_KIND: u8 = 1;
const MAX_ARGUMENTS: u32 = 32;
const MAX_HEADERS: u32 = 256;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Request {
    pub method: String,
    pub url: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

struct Fields {
    arguments_end: usize,
    method: Option<u32>,
    url: Option<u32>,
    headers: Option<u32>,
    request: Request,
}

fn is_method(value: &str) -> bool {
    matches!(value, "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD")
}

fn read_string(cursor: &mut Cursor) -> String {
    let length = cursor.read32() as usize;
    cursor
        .take(length)
        .map(|text| String::from_utf8_lossy(text).into_owned())
        .unwrap_or_default()
}

fn read_headers(cursor: &mut Cursor) -> Option<Vec<(String, String)>> {
    let count = cursor.read32();
    if count > MAX_HEADERS {
        return None;
    }
    let mut headers = Vec::new();
    for _ in 0..count {
        if cursor.failed() || cursor.read8() != Tag::Array || cursor.read32() != HEADER_PAIR || cursor.read8() != Tag::String {
            return None;
        }
        let name = read_string(cursor).to_ascii_lowercase();
        if cursor.read8() != Tag::String {
            return None;
        }
        headers.push((name, read_string(cursor)));
    }
    (!cursor.failed()).then_some(headers)
}

pub fn path_of(url: &str) -> String {
    let Some(scheme) = url.find("://") else { return "/".to_owned() };
    let Some(start) = url[scheme + 3..].find('/').map(|found| scheme + 3 + found) else {
        return "/".to_owned();
    };
    let end = url[start..].find(['?', '#']).map_or(url.len(), |found| start + found);
    url[start..end].to_owned()
}

pub fn host_of(url: &str) -> String {
    let Some(scheme) = url.find("://") else { return String::new() };
    let start = scheme + 3;
    let end = url[start..].find(['/', '?', '#', ':']).map_or(url.len(), |found| start + found);
    url[start..end].to_ascii_lowercase()
}

fn read(frame: &[u8]) -> Option<Fields> {
    if frame.len() < HEADER || frame[0] != CALL_KIND {
        return None;
    }
    let arguments_length = u32::from_le_bytes(frame[12..16].try_into().ok()?) as usize;
    if arguments_length == 0 || arguments_length > frame.len() - HEADER {
        return None;
    }
    let arguments_end = HEADER + arguments_length;
    let mut cursor = Cursor::new(&frame[..arguments_end], HEADER);
    if cursor.read8() != Tag::Map {
        return None;
    }
    let count = cursor.read32();
    if count > MAX_ARGUMENTS {
        return None;
    }
    let mut fields = Fields {
        arguments_end,
        method: None,
        url: None,
        headers: None,
        request: Request::default(),
    };
    for index in 0..count {
        if cursor.failed() {
            break;
        }
        cursor.read16();
        let tag = cursor.read8();
        match Tag::parse(tag) {
            Some(Tag::String) => {
                let value = read_string(&mut cursor);
                if fields.method.is_none() && is_method(&value) {
                    fields.method = Some(index);
                    fields.request.method = value;
                } else if fields.url.is_none() && value.starts_with("https://") {
                    fields.url = Some(index);
                    fields.request.url = value;
                }
            }
            Some(Tag::Array) if fields.headers.is_none() => {
                let mut probe = Cursor::new(&frame[..arguments_end], cursor.offset());
                if let Some(headers) = read_headers(&mut probe) {
                    fields.headers = Some(index);
                    fields.request.headers = headers;
                    cursor.skip(probe.offset() - cursor.offset());
                } else {
                    cursor.skip_value(7, 0);
                }
            }
            _ => cursor.skip_value(tag, 0),
        }
    }
    if cursor.failed() || fields.method.is_none() || fields.url.is_none() || fields.headers.is_none() {
        return None;
    }
    fields.request.path = path_of(&fields.request.url);
    fields.request.body = frame[arguments_end..].to_vec();
    Some(fields)
}

pub fn parse(frame: &[u8]) -> Option<Request> {
    read(frame).map(|fields| fields.request)
}

fn put_string(out: &mut Vec<u8>, value: &str) {
    out.push(6);
    out.extend_from_slice(&(value.len() as u32).to_le_bytes());
    out.extend_from_slice(value.as_bytes());
}

pub fn rebuild(frame: &[u8], request: &Request) -> Vec<u8> {
    let Some(fields) = read(frame) else { return frame.to_vec() };
    let mut cursor = Cursor::new(&frame[..fields.arguments_end], HEADER + 1);
    let count = cursor.read32();
    let mut arguments = vec![8];
    arguments.extend_from_slice(&count.to_le_bytes());
    for index in 0..count {
        if cursor.failed() {
            break;
        }
        let key = cursor.read16();
        let start = cursor.offset();
        let tag = cursor.read8();
        cursor.skip_value(tag, 0);
        arguments.extend_from_slice(&key.to_le_bytes());
        if Some(index) == fields.method {
            put_string(&mut arguments, &request.method);
        } else if Some(index) == fields.url {
            put_string(&mut arguments, &request.url);
        } else if Some(index) == fields.headers {
            arguments.push(7);
            arguments.extend_from_slice(&(request.headers.len() as u32).to_le_bytes());
            for (name, value) in &request.headers {
                arguments.push(7);
                arguments.extend_from_slice(&2u32.to_le_bytes());
                put_string(&mut arguments, name);
                put_string(&mut arguments, value);
            }
        } else {
            arguments.extend_from_slice(&frame[start..cursor.offset()]);
        }
    }
    if cursor.failed() {
        return frame.to_vec();
    }
    let mut out = frame[..12].to_vec();
    out.extend_from_slice(&(arguments.len() as u32).to_le_bytes());
    out.extend_from_slice(&arguments);
    out.extend_from_slice(&request.body);
    out
}

fn frames_of(data: &[u8]) -> Vec<Vec<u8>> {
    let mut entries = Vec::new();
    let mut at = 0;
    while at + 4 <= data.len() {
        let length = u32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]]) as usize;
        at += 4;
        if length > data.len() - at {
            break;
        }
        entries.push(data[at..at + length].to_vec());
        at += length;
    }
    entries
}

fn request_of(frames: &[Vec<u8>]) -> Option<Request> {
    let [method, url, path, body, headers @ ..] = frames else {
        return None;
    };
    let text = |value: &Vec<u8>| String::from_utf8_lossy(value).into_owned();
    Some(Request {
        method: text(method),
        url: text(url),
        path: text(path),
        body: body.clone(),
        headers: headers
            .as_chunks::<2>()
            .0
            .iter()
            .map(|[name, value]| (text(name), text(value)))
            .collect(),
    })
}

fn frames_from(request: &Request) -> Vec<Vec<u8>> {
    let mut frames = vec![
        request.method.clone().into_bytes(),
        request.url.clone().into_bytes(),
        request.path.clone().into_bytes(),
        request.body.clone(),
    ];
    for (name, value) in &request.headers {
        frames.push(name.clone().into_bytes());
        frames.push(value.clone().into_bytes());
    }
    frames
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_host_request_parse(frame: *const c_char, length: usize) -> AstralRustString {
    parse(bytes(frame, length)).map_or(
        AstralRustString {
            bytes: core::ptr::null_mut(),
            length: 0,
        },
        |request| AstralRustString::frames(&frames_from(&request)),
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_host_request_rebuild(
    frame: *const c_char,
    length: usize,
    request: *const c_char,
    request_length: usize,
) -> AstralRustString {
    let frame = bytes(frame, length);
    let rebuilt = request_of(&frames_of(bytes(request, request_length))).map_or_else(|| frame.to_vec(), |request| rebuild(frame, &request));
    AstralRustString::new(rebuilt)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_url_host(url: *const c_char, length: usize) -> AstralRustString {
    AstralRustString::new(host_of(&String::from_utf8_lossy(bytes(url, length))).into_bytes())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_url_path(url: *const c_char, length: usize) -> AstralRustString {
    AstralRustString::new(path_of(&String::from_utf8_lossy(bytes(url, length))).into_bytes())
}
