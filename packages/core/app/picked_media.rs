use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{Value, json};

const FETCH_BATCH: u64 = 64;
const FRESH: Duration = Duration::from_secs(20);
const TOKEN_LIMIT: usize = 256;
const FILE_LIMIT: usize = 64;
const VIDEO_HOST: &str = "media.discordapp.net";
const VIDEO_PATH: &str = "/astral-picked/";
const VIDEO_EXTENSION: &str = ".mp4";
const VIDEO: &str = "video";
const RANGE_PREFIX: &str = "bytes=";
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0100_0000_01b3;

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Entry {
    #[serde(default = "unknown_size")]
    pub size: i64,
    #[serde(default)]
    pub duration: f64,
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub url: Option<String>,
}

const fn unknown_size() -> i64 {
    -1
}

impl Entry {
    fn shape(&self) -> (i64, u64, &str) {
        (self.size, self.duration.to_bits(), &self.kind)
    }
}

#[derive(Default)]
struct State {
    entries: Vec<Entry>,
    fetched_at: Option<Instant>,
    fetching: bool,
    tokens: HashMap<String, String>,
    files: HashMap<String, String>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with_state<T>(work: impl FnOnce(&mut State) -> T) -> Option<T> {
    let mut guard = STATE.lock().ok()?;
    Some(work(guard.get_or_insert_with(State::default)))
}

fn fetch_limit(count: u64, slot: u64) -> u64 {
    count.max((slot / FETCH_BATCH + 1) * FETCH_BATCH)
}

fn needs_fetch(state: &State, slot: u64, now: Instant) -> bool {
    let count = state.entries.len() as u64;
    let stale = state.fetched_at.is_none_or(|fetched| now.duration_since(fetched) > FRESH);
    let past_batch = slot >= count && count.is_multiple_of(FETCH_BATCH);
    (stale || past_batch) && !state.fetching
}

pub fn entry(slot: u64, fetch: impl FnOnce(u64)) -> Option<Entry> {
    let (entry, limit) = with_state(|state| {
        let wanted = needs_fetch(state, slot, Instant::now()).then(|| {
            state.fetching = true;
            fetch_limit(state.entries.len() as u64, slot)
        });
        (
            usize::try_from(slot).ok().and_then(|index| state.entries.get(index).cloned()),
            wanted,
        )
    })?;
    if let Some(limit) = limit {
        fetch(limit);
    }
    entry
}

pub fn describe(entry: &Entry) -> Value {
    json!({ "size": entry.size, "duration": entry.duration, "type": entry.kind })
}

pub fn fetched(entries: &str) -> bool {
    let parsed: Vec<Entry> = serde_json::from_str(entries).unwrap_or_default();
    with_state(|state| {
        let changed = parsed.iter().map(Entry::shape).ne(state.entries.iter().map(Entry::shape));
        state.entries = parsed;
        state.fetched_at = Some(Instant::now());
        state.fetching = false;
        changed
    })
    .unwrap_or(false)
}

fn token(source: &str) -> String {
    let hash = source
        .bytes()
        .fold(FNV_OFFSET, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(FNV_PRIME));
    format!("{hash:016x}")
}

pub fn video_url(slot: u64) -> Option<String> {
    with_state(|state| {
        let entry = state.entries.get(usize::try_from(slot).ok()?)?.clone();
        if entry.kind != VIDEO {
            return None;
        }
        if let Some(url) = entry.url {
            return Some(url);
        }
        let token = token(&entry.source);
        if state.tokens.len() >= TOKEN_LIMIT {
            state.tokens.clear();
        }
        state.tokens.insert(token.clone(), entry.source);
        Some(format!("https://{VIDEO_HOST}{VIDEO_PATH}{token}{VIDEO_EXTENSION}"))
    })?
}

fn token_of(url: &str) -> Option<&str> {
    let rest = url.strip_prefix("https://")?.strip_prefix(VIDEO_HOST)?.strip_prefix(VIDEO_PATH)?;
    let name = rest.split(['?', '#']).next()?;
    Some(name.strip_suffix(VIDEO_EXTENSION).unwrap_or(name))
}

pub fn is_video_url(url: &str) -> bool {
    token_of(url).is_some()
}

pub enum VideoFile {
    Cached(String),
    Source(String),
    Unknown,
}

pub fn video_file(url: &str) -> VideoFile {
    let Some(token) = token_of(url) else { return VideoFile::Unknown };
    with_state(|state| {
        if let Some(file) = state.files.get(token) {
            return VideoFile::Cached(file.clone());
        }
        state
            .tokens
            .get(token)
            .map_or(VideoFile::Unknown, |source| VideoFile::Source(source.clone()))
    })
    .unwrap_or(VideoFile::Unknown)
}

pub fn remember_video_file(url: &str, file: &str) {
    let Some(token) = token_of(url) else { return };
    with_state(|state| {
        if state.files.len() >= FILE_LIMIT {
            state.files.clear();
        }
        state.files.insert(token.to_owned(), file.to_owned());
    });
}

pub fn byte_range(header: &str, length: u64) -> Option<(u64, u64)> {
    let bounds = header.strip_prefix(RANGE_PREFIX)?;
    if length == 0 {
        return None;
    }
    let (first, last) = bounds.split_once('-')?;
    let number = |text: &str| text.trim().parse::<u64>().ok();
    let (start, end) = match (first.is_empty(), last.is_empty()) {
        (true, false) => (length - number(last)?.min(length), length - 1),
        (false, _) => (number(first)?, if last.is_empty() { length - 1 } else { number(last)? }),
        (true, true) => return None,
    };
    if start >= length || end < start {
        return None;
    }
    let end = end.min(length - 1);
    Some((start, end - start + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_byte_ranges() {
        assert_eq!(byte_range("bytes=0-99", 1000), Some((0, 100)));
        assert_eq!(byte_range("bytes=900-", 1000), Some((900, 100)));
        assert_eq!(byte_range("bytes=-100", 1000), Some((900, 100)));
        assert_eq!(byte_range("bytes=990-2000", 1000), Some((990, 10)));
        assert_eq!(byte_range("bytes=1000-", 1000), None);
        assert_eq!(byte_range("items=0-1", 1000), None);
        assert_eq!(byte_range("bytes=0-1", 0), None);
    }

    #[test]
    fn fetches_in_batches_while_stale() {
        let now = Instant::now();
        let mut state = State::default();
        assert!(needs_fetch(&state, 0, now));
        state.fetched_at = Some(now);
        state.entries = vec![
            Entry {
                size: 1,
                duration: 0.0,
                kind: "image".into(),
                source: String::new(),
                url: None
            };
            64
        ];
        assert!(!needs_fetch(&state, 3, now));
        assert!(needs_fetch(&state, 64, now));
        assert_eq!(fetch_limit(64, 64), 128);
        state.fetching = true;
        assert!(!needs_fetch(&state, 64, now));
    }

    #[test]
    fn video_tokens_are_stable_and_recognised() {
        assert_eq!(token("abc"), token("abc"));
        assert_ne!(token("abc"), token("abd"));
        let url = format!("https://{VIDEO_HOST}{VIDEO_PATH}{}{VIDEO_EXTENSION}", token("abc"));
        assert!(is_video_url(&url));
        assert!(!is_video_url("https://media.discordapp.net/attachments/1.mp4"));
    }
}
