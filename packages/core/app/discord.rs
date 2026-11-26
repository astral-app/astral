use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::mpsc::{Sender, channel};
use std::time::Duration;

use serde_json::Value;

use super::host::{self, Reply, Request};

const WAIT: Duration = Duration::from_secs(20);
const METHODS: [&str; 5] = ["DELETE", "GET", "PATCH", "POST", "PUT"];

static WAITERS: Mutex<Option<HashMap<u64, Sender<Value>>>> = Mutex::new(None);

fn valid(method: &str, path: &str) -> bool {
    METHODS.contains(&method) && path.starts_with('/') && !path.starts_with("//") && !path.contains("..")
}

pub fn send(request: u64, method: &str, path: &str, body: &str) -> bool {
    valid(method, path)
        && matches!(
            host::request(Request::DiscordRequest {
                request,
                method,
                path,
                body,
            }),
            Reply::Done | Reply::Flag(true)
        )
}

fn with_waiters<T>(work: impl FnOnce(&mut HashMap<u64, Sender<Value>>) -> T) -> T {
    let mut guard = WAITERS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    work(guard.get_or_insert_with(HashMap::new))
}

pub fn complete(request: u64, response: &Value) -> bool {
    let Some(waiter) = with_waiters(|waiters| waiters.remove(&request)) else {
        return false;
    };
    let _ = waiter.send(response.clone());
    true
}

pub fn get(path: &str) -> (i32, Vec<u8>) {
    let request = crate::lua::host::next_request();
    let (sender, receiver) = channel();
    with_waiters(|waiters| waiters.insert(request, sender));
    if !send(request, "GET", path, "") {
        with_waiters(|waiters| waiters.remove(&request));
        return (0, Vec::new());
    }
    let Ok(response) = receiver.recv_timeout(WAIT) else {
        with_waiters(|waiters| waiters.remove(&request));
        return (0, Vec::new());
    };
    let status = response.get("status").and_then(Value::as_i64).unwrap_or(0) as i32;
    let body = response.get("body").and_then(Value::as_str).unwrap_or_default().as_bytes().to_vec();
    (status, body)
}
