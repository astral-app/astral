use core::ffi::{c_char, c_int};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Condvar, Mutex, Once};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::bridge::{AstralRustString, bytes};
use crate::{discord_headers, session};

const FACES: usize = 5;
const MAX_ENTRIES: usize = 512;
const RETRY_AFTER: Duration = Duration::from_secs(3);
const MAX_RATE_LIMIT_WAIT: Duration = Duration::from_secs(10);
const REFRESH_COOLDOWN: Duration = Duration::from_secs(10);

pub type Fetcher = unsafe extern "C" fn(*const c_char, usize, *const c_char, usize, *mut AstralRustString) -> c_int;

#[derive(Default)]
struct Entry {
    count: u64,
    users: Option<Value>,
    failed: Option<Instant>,
    fetched: Option<Instant>,
}

struct Job {
    key: String,
    path: String,
    count: u64,
    not_before: Option<Instant>,
}

#[derive(Default)]
struct State {
    entries: HashMap<String, Entry>,
    in_flight: HashSet<String>,
    jobs: VecDeque<Job>,
    fetcher: Option<Fetcher>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static WAKE: Condvar = Condvar::new();
static WORKER: Once = Once::new();

fn with_state<T>(work: impl FnOnce(&mut State) -> T) -> T {
    let mut guard = STATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    work(guard.get_or_insert_with(State::default))
}

pub fn encoded(value: &str) -> String {
    value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

pub fn users(body: &[u8]) -> Option<Value> {
    let parsed: Value = serde_json::from_slice(body).ok()?;
    let list: Vec<Value> = parsed
        .as_array()?
        .iter()
        .filter_map(|user| {
            let id = user.get("id")?.as_str()?;
            let mut entry = json!({ "id": id });
            for field in ["username", "global_name", "avatar"] {
                if let Some(text) = user.get(field).and_then(Value::as_str) {
                    entry[field] = json!(text);
                }
            }
            Some(entry)
        })
        .take(FACES)
        .collect();
    (!list.is_empty()).then_some(Value::Array(list))
}

pub fn rate_limit_wait(body: &[u8]) -> Duration {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|parsed| parsed.get("retry_after").and_then(Value::as_f64))
        .filter(|seconds| seconds.is_finite() && *seconds > 0.0)
        .map_or(Duration::ZERO, |seconds| Duration::from_secs_f64(seconds).min(MAX_RATE_LIMIT_WAIT))
}

fn store(key: &str, count: u64, found: Option<Value>) {
    with_state(|state| {
        state.in_flight.remove(key);
        if state.entries.len() >= MAX_ENTRIES {
            state.entries.clear();
        }
        let failed = found.is_none().then(Instant::now);
        let users = found.or_else(|| state.entries.get(key).and_then(|entry| entry.users.clone()));
        state.entries.insert(
            key.to_owned(),
            Entry {
                count,
                users,
                failed,
                fetched: Some(Instant::now()),
            },
        );
    });
}

fn header_frames(token: &[u8]) -> Vec<u8> {
    let mut frames = Vec::new();
    let mut push = |value: &[u8]| {
        frames.extend_from_slice(&(value.len() as u32).to_le_bytes());
        frames.extend_from_slice(value);
    };
    push(b"Authorization");
    push(token);
    for (name, value) in discord_headers::headers() {
        push(name.as_bytes());
        push(value.as_bytes());
    }
    frames
}

fn fetch(fetcher: Fetcher, url: &str, token: &[u8]) -> (c_int, Vec<u8>) {
    let mut body = AstralRustString {
        bytes: core::ptr::null_mut(),
        length: 0,
    };
    let headers = header_frames(token);
    let status = unsafe { fetcher(url.as_ptr().cast(), url.len(), headers.as_ptr().cast(), headers.len(), &mut body) };
    let data = if body.bytes.is_null() {
        Vec::new()
    } else {
        unsafe { core::slice::from_raw_parts(body.bytes, body.length) }.to_vec()
    };
    unsafe { crate::bridge::astral_rust_string_free(body) };
    (status, data)
}

fn work() {
    loop {
        let (job, fetcher) = {
            let mut guard = STATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            loop {
                let state = guard.get_or_insert_with(State::default);
                let now = Instant::now();
                if let Some(index) = state.jobs.iter().position(|job| job.not_before.is_none_or(|moment| moment <= now)) {
                    let job = state.jobs.remove(index).expect("ready job");
                    break (job, state.fetcher);
                }
                let earliest = state.jobs.iter().filter_map(|job| job.not_before).min();
                guard = match earliest {
                    Some(moment) => {
                        WAKE.wait_timeout(guard, moment.saturating_duration_since(now))
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .0
                    }
                    None => WAKE.wait(guard).unwrap_or_else(|poisoned| poisoned.into_inner()),
                };
            }
        };
        let token = session::current();
        let Some(fetcher) = fetcher.filter(|_| !token.is_empty()) else {
            store(&job.key, job.count, None);
            continue;
        };
        let url = format!("https://discord.com/api/v{}{}", discord_headers::api_version(), job.path);
        let (status, body) = fetch(fetcher, &url, &token);
        store(
            &job.key,
            job.count,
            if i64::from(status) == crate::http_status::OK {
                users(&body)
            } else {
                None
            },
        );
        if i64::from(status) == crate::http_status::TOO_MANY_REQUESTS {
            std::thread::sleep(rate_limit_wait(&body));
        }
    }
}

pub fn lookup(channel: &str, message: &str, emoji: &str, burst: bool, count: u64) -> (Option<Value>, bool) {
    if channel.is_empty() || message.is_empty() || emoji.is_empty() || count == 0 {
        return (None, false);
    }
    WORKER.call_once(|| {
        std::thread::spawn(work);
    });
    let key = format!("{channel}:{message}:{emoji}:{}", u8::from(burst));
    with_state(|state| {
        let entry = state.entries.get(&key);
        let stale = entry.is_none_or(|entry| entry.count != count);
        let failed = entry.and_then(|entry| entry.failed);
        let cooling_down = !stale && failed.is_some_and(|failed| failed.elapsed() < RETRY_AFTER);
        let fetch = (stale || (failed.is_some() && !cooling_down)) && !state.in_flight.contains(&key);
        let users = entry.and_then(|entry| entry.users.clone());
        let fresh = !fetch && users.is_some() && entry.is_some_and(|entry| entry.count == count);
        let not_before = entry
            .and_then(|entry| entry.fetched)
            .map(|fetched| fetched + REFRESH_COOLDOWN)
            .filter(|moment| *moment > Instant::now());
        if fetch {
            state.in_flight.insert(key.clone());
            state.jobs.push_back(Job {
                path: format!(
                    "/channels/{channel}/messages/{message}/reactions/{}?limit={FACES}&type={}",
                    encoded(emoji),
                    u8::from(burst)
                ),
                key,
                count,
                not_before,
            });
            WAKE.notify_one();
        }
        (users, fresh)
    })
}

pub fn invalidate() {
    with_state(|state| {
        for entry in state.entries.values_mut() {
            entry.count = 0;
            entry.fetched = None;
        }
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_reactors_invalidate() {
    invalidate();
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_reactors_set_fetcher(fetcher: Option<Fetcher>) {
    with_state(|state| state.fetcher = fetcher);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_reactors_lookup(
    channel: *const c_char,
    channel_length: usize,
    message: *const c_char,
    message_length: usize,
    emoji: *const c_char,
    emoji_length: usize,
    burst: c_int,
    count: u64,
    fresh: *mut c_int,
) -> AstralRustString {
    let text = |data, length| String::from_utf8_lossy(bytes(data, length)).into_owned();
    let (users, current) = lookup(
        &text(channel, channel_length),
        &text(message, message_length),
        &text(emoji, emoji_length),
        burst != 0,
        count,
    );
    if let Some(fresh) = unsafe { fresh.as_mut() } {
        *fresh = c_int::from(current);
    }
    users.map_or(
        AstralRustString {
            bytes: core::ptr::null_mut(),
            length: 0,
        },
        |users| AstralRustString::new(users.to_string().into_bytes()),
    )
}
