use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::sync::Mutex;

use serde::Deserialize;

const HTTPS_PORT: u16 = 443;
const HTTP_PORT: u16 = 80;
const ROOT: &str = "/";
const COOKIE_SEPARATOR: &str = "; ";
const BROWSER_LOGIN: &str = "https://discord.com/login#astral-browser-login=";
const STATE_BYTES: usize = 16;
const RANDOM_SOURCE: &str = "/dev/urandom";

struct Session {
    callback: String,
    initial: HashSet<String>,
}

static SESSIONS: Mutex<Option<HashMap<u64, Session>>> = Mutex::new(None);

fn with_sessions<T>(work: impl FnOnce(&mut HashMap<u64, Session>) -> T) -> Option<T> {
    let mut guard = SESSIONS.lock().ok()?;
    Some(work(guard.get_or_insert_with(HashMap::new)))
}

#[derive(Debug, PartialEq)]
struct Address<'a> {
    scheme: String,
    host: String,
    port: Option<u16>,
    path: &'a str,
}

fn address(url: &str) -> Option<Address<'_>> {
    let (scheme, rest) = url.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = rest[..end].rsplit('@').next().unwrap_or_default();
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !host.ends_with(']') || authority.starts_with('[') => (host, Some(port.parse().ok()?)),
        _ => (authority, None),
    };
    let tail = &rest[end..];
    let path_end = tail.find(['?', '#']).unwrap_or(tail.len());
    let default = match scheme.as_str() {
        "https" => Some(HTTPS_PORT),
        "http" => Some(HTTP_PORT),
        _ => None,
    };
    Some(Address {
        port: port.or(default),
        scheme,
        host: host.to_ascii_lowercase(),
        path: &tail[..path_end],
    })
}

fn same_path(left: &str, right: &str) -> bool {
    let normal = |path: &str| if path.is_empty() { ROOT.to_owned() } else { path.to_owned() };
    normal(left) == normal(right)
}

pub fn matches_callback(url: &str, callback: &str) -> bool {
    let (Some(actual), Some(expected)) = (address(url), address(callback)) else {
        return false;
    };
    actual.scheme == expected.scheme
        && actual.host == expected.host
        && actual.port == expected.port
        && same_path(actual.path, expected.path)
}

fn header_values(header: &str) -> impl Iterator<Item = String> + '_ {
    header
        .split(';')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

pub fn begin(request: u64, callback: &str) {
    with_sessions(|sessions| {
        sessions.insert(
            request,
            Session {
                callback: callback.to_owned(),
                initial: HashSet::new(),
            },
        );
    });
}

pub fn note_initial_cookies(request: u64, header: &str) {
    with_sessions(|sessions| {
        if let Some(session) = sessions.get_mut(&request) {
            session.initial = header_values(header).collect();
        }
    });
}

pub fn is_callback(request: u64, url: &str) -> bool {
    with_sessions(|sessions| {
        sessions
            .get(&request)
            .is_some_and(|session| matches_callback(url, &session.callback))
    })
    .unwrap_or(false)
}

pub fn new_cookies(request: u64, header: &str) -> String {
    with_sessions(|sessions| {
        let Some(session) = sessions.get(&request) else {
            return String::new();
        };
        header_values(header)
            .filter(|value| !session.initial.contains(value))
            .collect::<Vec<_>>()
            .join(COOKIE_SEPARATOR)
    })
    .unwrap_or_default()
}

pub fn end(request: u64) {
    with_sessions(|sessions| sessions.remove(&request));
}

#[derive(Deserialize)]
pub struct Cookie {
    name: String,
    value: String,
    #[serde(default)]
    domain: String,
    #[serde(default)]
    path: String,
}

fn path_matches(path: &str, cookie_path: &str) -> bool {
    path == cookie_path
        || (path.starts_with(cookie_path) && (cookie_path.ends_with('/') || path.as_bytes().get(cookie_path.len()) == Some(&b'/')))
}

pub fn cookie_header(cookies: &str, url: &str) -> String {
    let Some(target) = address(url) else { return String::new() };
    let path = if target.path.is_empty() { ROOT } else { target.path };
    let parsed: Vec<Cookie> = serde_json::from_str(cookies).unwrap_or_default();
    parsed
        .iter()
        .filter(|cookie| cookie.domain.trim_matches('.').eq_ignore_ascii_case(&target.host))
        .filter(|cookie| path_matches(path, if cookie.path.is_empty() { ROOT } else { &cookie.path }))
        .map(|cookie| format!("{}={}", cookie.name, cookie.value))
        .collect::<Vec<_>>()
        .join(COOKIE_SEPARATOR)
}

pub fn browser_login_url() -> String {
    let mut bytes = [0u8; STATE_BYTES];
    if let Ok(mut random) = std::fs::File::open(RANDOM_SOURCE) {
        let _ = random.read_exact(&mut bytes);
    }
    let state: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("{BROWSER_LOGIN}{state}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_callbacks_by_origin_and_path() {
        assert!(matches_callback("https://example.com/cb?code=1", "https://example.com/cb"));
        assert!(matches_callback("https://EXAMPLE.com:443/cb", "https://example.com/cb"));
        assert!(matches_callback("https://example.com?x", "https://example.com/"));
        assert!(!matches_callback("https://example.com/cb2", "https://example.com/cb"));
        assert!(!matches_callback("http://example.com/cb", "https://example.com/cb"));
        assert!(!matches_callback("https://evil.com/cb", "https://example.com/cb"));
        assert!(!matches_callback("https://example.com:8443/cb", "https://example.com/cb"));
        assert!(matches_callback("myapp://auth/done", "myapp://auth/done"));
    }

    #[test]
    fn reports_only_cookies_set_during_the_session() {
        begin(7, "https://example.com/cb");
        note_initial_cookies(7, "a=1; b=2");
        assert!(is_callback(7, "https://example.com/cb?ok"));
        assert_eq!(new_cookies(7, "a=1; b=3; c=4"), "b=3; c=4");
        end(7);
        assert!(!is_callback(7, "https://example.com/cb"));
    }

    #[test]
    fn selects_cookies_for_a_url() {
        let cookies = r#"[{"name":"a","value":"1","domain":".example.com","path":"/"},{"name":"b","value":"2","domain":"example.com","path":"/api"},{"name":"c","value":"3","domain":"other.com","path":"/"}]"#;
        assert_eq!(cookie_header(cookies, "https://example.com/api/v1"), "a=1; b=2");
        assert_eq!(cookie_header(cookies, "https://example.com/apiv"), "a=1");
    }

    #[test]
    fn browser_login_urls_carry_a_fresh_state() {
        let first = browser_login_url();
        assert!(first.starts_with(BROWSER_LOGIN));
        assert_ne!(first, browser_login_url());
    }
}
