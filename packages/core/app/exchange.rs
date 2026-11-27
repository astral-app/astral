use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

use super::network::{self, Probe};
use crate::lua::host::http::{HttpRequest, HttpResponse};
use crate::network::host_requests::{host_of, path_of};

pub const BLOCK: u32 = 1;
pub const MARK_STAFF: u32 = 1 << 1;
pub const INTERCEPT: u32 = 1 << 2;
pub const TRANSFORM_MESSAGE: u32 = 1 << 3;
pub const REWRITE_IMAGE: u32 = 1 << 4;
pub const PLAIN_GATEWAY: u32 = 1 << 5;
pub const EVERY_STAGE: u32 = BLOCK | MARK_STAFF | INTERCEPT | TRANSFORM_MESSAGE | REWRITE_IMAGE;

const PENDING_CAPACITY: usize = 512;
const OK: i64 = 200;
const JSON_TYPE: &str = "json";
const CONTENT_TYPE: &str = "content-type";
const STALE_REQUEST_HEADERS: [&str; 2] = ["content-length", "transfer-encoding"];
const STALE_RESPONSE_HEADERS: [&str; 2] = ["content-length", "content-encoding"];
const GATEWAY_PREFIX: &str = "gateway";
const GATEWAY_SUFFIX: &str = ".discord.gg";

struct Pending {
    request: HttpRequest,
    mark_staff: bool,
    intercepted: bool,
}

static NEXT_EXCHANGE: AtomicU64 = AtomicU64::new(1);
static PENDING: Mutex<BTreeMap<u64, Pending>> = Mutex::new(BTreeMap::new());

fn is_gateway(host: &str) -> bool {
    host.starts_with(GATEWAY_PREFIX) && host.ends_with(GATEWAY_SUFFIX)
}

pub fn plan_flags(method: &str, url: &str, json_body: bool) -> u32 {
    let (host, path) = (host_of(url), path_of(url));
    let plan = network::plan(&Probe {
        method,
        url,
        host: &host,
        path: &path,
        headers: "",
        json_body,
    });
    [
        (plan.block, BLOCK),
        (plan.mark_staff, MARK_STAFF),
        (plan.intercept, INTERCEPT),
        (plan.transform_message, TRANSFORM_MESSAGE),
        (plan.rewrite_image, REWRITE_IMAGE),
        (is_gateway(&host) && network::wants_uncompressed_gateway(), PLAIN_GATEWAY),
    ]
    .into_iter()
    .filter(|(on, _)| *on)
    .fold(0, |flags, (_, bit)| flags | bit)
}

fn without_headers(headers: &mut Vec<(String, String)>, names: &[&str]) {
    headers.retain(|(name, _)| !names.iter().any(|stale| name.eq_ignore_ascii_case(stale)));
}

fn has_json_body(request: &HttpRequest) -> bool {
    request
        .headers
        .iter()
        .any(|(name, value)| name.eq_ignore_ascii_case(CONTENT_TYPE) && value.to_ascii_lowercase().contains(JSON_TYPE))
}

fn remember(pending: Pending) -> u64 {
    let id = NEXT_EXCHANGE.fetch_add(1, Ordering::Relaxed);
    let Ok(mut table) = PENDING.lock() else { return 0 };
    while table.len() >= PENDING_CAPACITY {
        table.pop_first();
    }
    table.insert(id, pending);
    id
}

fn take(exchange: u64) -> Option<Pending> {
    PENDING.lock().ok()?.remove(&exchange)
}

pub fn begin(request: &str, stages: u32) -> Value {
    let parsed: Value = serde_json::from_str(request).unwrap_or(Value::Null);
    let mut request = network::request_of(&parsed);
    if request.path.is_empty() {
        request.path = path_of(&request.url);
    }
    let host = host_of(&request.url);
    let flags = plan_flags(&request.method, &request.url, has_json_body(&request)) & stages;
    if flags & BLOCK != 0 {
        super::note_blocked_request();
        return json!({ "block": true });
    }
    let mut changed = false;
    if flags & REWRITE_IMAGE != 0
        && let Some(url) = network::rewrite_image(&request.method, &host, &request.url)
    {
        request.url = url;
        changed = true;
    }
    if flags & TRANSFORM_MESSAGE != 0
        && let Some(body) = network::transform_outgoing(&request.method, &host, &request.path, &request.body)
    {
        request.body = body;
        without_headers(&mut request.headers, &STALE_REQUEST_HEADERS);
        changed = true;
    }
    let intercepted = flags & INTERCEPT != 0;
    if intercepted {
        let decision = super::intercept_http_request(&mut request);
        if let Some(response) = decision.response {
            return json!({ "respond": network::response_json(&response) });
        }
        changed |= decision.changed;
    }
    let forwarded = changed.then(|| network::request_json(&request));
    let mark_staff = flags & MARK_STAFF != 0;
    let exchange = (mark_staff || intercepted).then(|| {
        remember(Pending {
            request,
            mark_staff,
            intercepted,
        })
    });
    json!({ "request": forwarded, "exchange": exchange })
}

pub fn finish(exchange: u64, response: &str) -> Option<Value> {
    let pending = take(exchange)?;
    let mut response: HttpResponse = network::response_of(&serde_json::from_str(response).unwrap_or(Value::Null));
    let mut changed = false;
    if pending.mark_staff
        && response.status == OK
        && let Some(body) = network::mark_staff_user(&response.body)
    {
        response.body = body;
        without_headers(&mut response.headers, &STALE_RESPONSE_HEADERS);
        changed = true;
    }
    if pending.intercepted && super::intercept_http_response(&pending.request, &mut response) {
        without_headers(&mut response.headers, &STALE_RESPONSE_HEADERS);
        changed = true;
    }
    changed.then(|| network::response_json(&response))
}

pub fn abandon(exchange: u64) {
    take(exchange);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_exchanges_stay_bounded() {
        for _ in 0..PENDING_CAPACITY + 10 {
            remember(Pending {
                request: network::request_of(&Value::Null),
                mark_staff: false,
                intercepted: false,
            });
        }
        assert!(PENDING.lock().unwrap().len() <= PENDING_CAPACITY);
    }

    #[test]
    fn unknown_exchanges_finish_unchanged() {
        assert!(finish(u64::MAX, "{}").is_none());
    }

    #[test]
    fn gateway_hosts_are_recognised() {
        assert!(is_gateway("gateway-us-east1-b.discord.gg"));
        assert!(!is_gateway("discord.com"));
    }
}
