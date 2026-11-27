use serde_json::{Value, json};

const UNAVAILABLE: &str = "network requests are not built into this binary";

pub fn cancel(_request: u64) {}

pub fn perform(_options: &Value) -> Value {
    json!({ "ok": false, "status": 0, "error": UNAVAILABLE })
}

pub fn start(_request: u64, _options: Value, _complete: fn(u64, &Value)) -> bool {
    false
}

pub fn fetch(_url: &str) -> (i32, Vec<u8>) {
    (0, Vec::new())
}
