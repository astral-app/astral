use core::ffi::c_char;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicU16, AtomicU64, Ordering};
use std::sync::{Mutex, Once};
use std::time::Duration;

use crate::bridge::{AstralRustString, bytes};

const EMPTY_RESPONSE: &[u8] = b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
const RESPONSE_LIMIT: usize = 64;

static PORT: AtomicU16 = AtomicU16::new(0);
static STARTED: Once = Once::new();
static NEXT_TOKEN: AtomicU64 = AtomicU64::new(1);
static RESPONSES: Mutex<BTreeMap<String, Vec<u8>>> = Mutex::new(BTreeMap::new());

fn registered_response(request: &[u8]) -> Option<Vec<u8>> {
    let text = String::from_utf8_lossy(request);
    let start = text.find(" /r/")? + 4;
    let rest = &text[start..];
    let token = &rest[..rest.find([' ', '?']).unwrap_or(rest.len())];
    RESPONSES.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).remove(token)
}

fn answer(mut client: TcpStream) {
    let _ = client.set_read_timeout(Some(Duration::from_secs(1)));
    let mut buffer = [0u8; 4096];
    let received = client.read(&mut buffer).unwrap_or(0);
    let request = buffer[..received].to_vec();
    let _ = client.set_nonblocking(true);
    while matches!(client.read(&mut buffer), Ok(count) if count > 0) {}
    let _ = client.set_nonblocking(false);
    let response = registered_response(&request);
    let _ = client.write_all(response.as_deref().unwrap_or(EMPTY_RESPONSE));
}

fn listen_on_loopback() {
    let Ok(listener) = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)) else {
        return;
    };
    let Ok(address) = listener.local_addr() else { return };
    PORT.store(address.port(), Ordering::Relaxed);
    std::thread::spawn(move || {
        for client in listener.incoming().flatten() {
            std::thread::spawn(move || answer(client));
        }
    });
}

pub fn start() -> u16 {
    STARTED.call_once(listen_on_loopback);
    PORT.load(Ordering::Relaxed)
}

pub fn url() -> String {
    match start() {
        0 => String::new(),
        port => format!("http://127.0.0.1:{port}/"),
    }
}

pub fn respond_with(status: i32, headers: &[(Vec<u8>, Vec<u8>)], body: &[u8]) -> Vec<u8> {
    let base = url();
    if base.is_empty() {
        return Vec::new();
    }
    let mut response = format!(
        "HTTP/1.1 {status} astral\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    )
    .into_bytes();
    let mut typed = false;
    for (name, value) in headers {
        if matches!(name.as_slice(), b"content-length" | b"connection" | b"content-encoding") {
            continue;
        }
        typed |= name == b"content-type";
        response.extend_from_slice(name);
        response.extend_from_slice(b": ");
        response.extend_from_slice(value);
        response.extend_from_slice(b"\r\n");
    }
    if !typed && !body.is_empty() {
        response.extend_from_slice(b"content-type: application/json\r\n");
    }
    response.extend_from_slice(b"\r\n");
    response.extend_from_slice(body);
    let token = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed).to_string();
    let mut responses = RESPONSES.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if responses.len() > RESPONSE_LIMIT {
        responses.pop_first();
    }
    responses.insert(token.clone(), response);
    format!("{base}r/{token}").into_bytes()
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_sink_start() -> u16 {
    start()
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_sink_url() -> AstralRustString {
    AstralRustString::new(url().into_bytes())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_sink_respond_with(
    status: i32,
    headers: *const c_char,
    headers_length: usize,
    body: *const c_char,
    body_length: usize,
) -> AstralRustString {
    let mut parts = bytes(headers, headers_length).split(|byte| *byte == 0);
    let mut pairs = Vec::new();
    while let (Some(name), Some(value)) = (parts.next(), parts.next()) {
        pairs.push((name.to_vec(), value.to_vec()));
    }
    AstralRustString::new(respond_with(status, &pairs, bytes(body, body_length)))
}
