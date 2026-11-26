use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde_json::{Map, Value, json};

const MAX_RESPONSE_BYTES: u64 = 1 << 20;
const MAX_FETCH_BYTES: u64 = 32 << 20;
const DEFAULT_TIMEOUT_MS: u64 = 15_000;
const FETCH_TIMEOUT_MS: u64 = 20_000;
const MAX_HEADERS: usize = 64;
const MAX_HEADER_VALUE: usize = 8_192;

static CANCELLED: Mutex<Option<HashSet<u64>>> = Mutex::new(None);

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .max_redirects(0)
        .http_status_as_error(false)
        .user_agent("astral")
        .build()
        .into()
}

fn shared_agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| agent(Duration::from_millis(FETCH_TIMEOUT_MS)))
}

fn with_cancelled<T>(work: impl FnOnce(&mut HashSet<u64>) -> T) -> T {
    let mut guard = CANCELLED.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    work(guard.get_or_insert_with(HashSet::new))
}

pub fn cancel(request: u64) {
    with_cancelled(|cancelled| cancelled.insert(request));
}

fn read_body(response: &mut ureq::http::Response<ureq::Body>, limit: u64) -> Result<(Vec<u8>, bool), String> {
    let mut body = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(limit + 1)
        .read_to_end(&mut body)
        .map_err(|error| error.to_string())?;
    let too_large = body.len() as u64 > limit;
    if too_large {
        body.clear();
    }
    Ok((body, too_large))
}

fn response_headers(response: &ureq::http::Response<ureq::Body>) -> Value {
    let mut headers: HashMap<String, String> = HashMap::new();
    for (name, value) in response.headers().iter().take(MAX_HEADERS) {
        let value = value.to_str().unwrap_or_default();
        headers
            .entry(name.as_str().to_owned())
            .and_modify(|joined| {
                joined.push_str(", ");
                joined.push_str(value);
            })
            .or_insert_with(|| value.to_owned());
    }
    let map: Map<String, Value> = headers
        .into_iter()
        .map(|(name, mut value)| {
            value.truncate(MAX_HEADER_VALUE);
            (name, Value::String(value))
        })
        .collect();
    Value::Object(map)
}

fn failure(url: &str, error: &str) -> Value {
    json!({ "status": 0, "ok": false, "url": url, "headers": {}, "body": "", "error": error })
}

pub fn perform(options: &Value) -> Value {
    let url = options.get("url").and_then(Value::as_str).unwrap_or_default();
    if !url.starts_with("https://") {
        return failure(url, "invalid https URL");
    }
    let method = options.get("method").and_then(Value::as_str).unwrap_or("GET");
    let timeout = options.get("timeout_ms").and_then(Value::as_u64).unwrap_or(DEFAULT_TIMEOUT_MS);
    let Ok(method) = ureq::http::Method::from_bytes(method.as_bytes()) else {
        return failure(url, "unsupported method");
    };
    let mut request = ureq::http::Request::builder().method(method).uri(url);
    let mut has_content_type = false;
    if let Some(headers) = options.get("headers").and_then(Value::as_object) {
        for (name, value) in headers {
            if let Some(value) = value.as_str() {
                has_content_type |= name.eq_ignore_ascii_case("content-type");
                request = request.header(name, value);
            }
        }
    }
    let body = options.get("body").and_then(Value::as_str).map(|body| body.as_bytes().to_vec());
    if body.is_some() && !has_content_type {
        request = request.header("content-type", "application/octet-stream");
    }
    let Ok(request) = request.body(body.unwrap_or_default()) else {
        return failure(url, "invalid request");
    };
    let mut response = match agent(Duration::from_millis(timeout)).run(request) {
        Ok(response) => response,
        Err(_) => return failure(url, "request failed"),
    };
    let status = response.status().as_u16();
    let headers = response_headers(&response);
    let (body, too_large) = match read_body(&mut response, MAX_RESPONSE_BYTES) {
        Ok(read) => read,
        Err(_) => return failure(url, "request failed"),
    };
    let mut result = json!({
        "status": status,
        "ok": (200..300).contains(&status) && !too_large,
        "url": url,
        "headers": headers,
    });
    match String::from_utf8(body) {
        Ok(text) => result["body"] = json!(text),
        Err(error) => {
            result["body"] = json!("");
            result["body_base64"] = json!(crate::lua::host::install::encode_base64(error.as_bytes()));
        }
    }
    if too_large {
        result["error"] = json!("response body exceeds 1 MiB");
    }
    result
}

pub fn start(request: u64, options: Value, complete: fn(u64, &Value)) -> bool {
    with_cancelled(|cancelled| cancelled.remove(&request));
    std::thread::Builder::new()
        .name("astral-http".into())
        .spawn(move || {
            let result = perform(&options);
            if !with_cancelled(|cancelled| cancelled.remove(&request)) {
                complete(request, &result);
            }
        })
        .is_ok()
}

pub fn fetch(url: &str) -> (i32, Vec<u8>) {
    let Ok(mut response) = shared_agent().get(url).call() else {
        return (0, Vec::new());
    };
    let status = i32::from(response.status().as_u16());
    match read_body(&mut response, MAX_FETCH_BYTES) {
        Ok((body, false)) => (status, body),
        _ => (0, Vec::new()),
    }
}
