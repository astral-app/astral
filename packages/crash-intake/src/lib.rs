use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use worker::{Context, Env, Request, Response, Result, RouteContext, Router, event, kv::KvStore};

const MAX_BODY: usize = 64 * 1024;
const MAX_TITLE: usize = 200;
const MAX_DETAIL: usize = 16 * 1024;
const MAX_FIELD: usize = 256;
const MAX_VERSIONS: usize = 20;
const HOURLY_LIMIT: u32 = 30;
const PENDING_BATCH: u64 = 50;
const KINDS: [&str; 3] = ["plugin", "crash", "hang"];
const PLATFORMS: [&str; 2] = ["ios", "android"];

#[derive(Serialize, Deserialize, Clone)]
struct Issue {
    number: u64,
    url: String,
}

#[derive(Serialize, Deserialize)]
struct Stored {
    report: Value,
    count: u64,
    versions: Vec<String>,
    issue: Option<Issue>,
}

#[derive(Deserialize)]
struct Synced {
    fingerprint: String,
    number: u64,
    url: String,
}

fn clipped(value: Option<&Value>, limit: usize) -> String {
    let text = value.and_then(Value::as_str).unwrap_or_default();
    text.chars().take(limit).collect()
}

fn valid_fingerprint(fingerprint: &str) -> bool {
    fingerprint.len() == 16 && fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn environment(source: Option<&Value>) -> Value {
    let mut cleaned = Map::new();
    for (key, value) in source
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .take(40)
    {
        let cleaned_value = match value {
            Value::String(text) => json!(text.chars().take(MAX_FIELD).collect::<String>()),
            Value::Number(_) | Value::Bool(_) => value.clone(),
            Value::Array(items) => json!(
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .take(64)
                    .map(|item| item.chars().take(MAX_FIELD).collect::<String>())
                    .collect::<Vec<_>>()
            ),
            _ => continue,
        };
        cleaned.insert(key.chars().take(64).collect(), cleaned_value);
    }
    Value::Object(cleaned)
}

fn cleaned_report(raw: &Value) -> Option<Value> {
    let kind = clipped(raw.get("kind"), 16);
    let fingerprint = clipped(raw.get("fingerprint"), 32);
    let title = clipped(raw.get("title"), MAX_TITLE);
    if !KINDS.contains(&kind.as_str()) || !valid_fingerprint(&fingerprint) || title.is_empty() {
        return None;
    }
    let environment = environment(raw.get("environment"));
    let platform = clipped(environment.get("platform"), 16);
    if !PLATFORMS.contains(&platform.as_str()) {
        return None;
    }
    Some(json!({
        "kind": kind,
        "fingerprint": fingerprint,
        "title": title,
        "detail": clipped(raw.get("detail"), MAX_DETAIL),
        "plugin": clipped(raw.get("plugin"), MAX_FIELD),
        "environment": environment,
    }))
}

fn report_key(fingerprint: &str) -> String {
    format!("report:{fingerprint}")
}

fn pending_key(fingerprint: &str) -> String {
    format!("pending:{fingerprint}")
}

fn error(message: &str, status: u16) -> Result<Response> {
    Ok(Response::from_json(&json!({ "error": message }))?.with_status(status))
}

async fn within_limit(store: &KvStore, request: &Request) -> Result<bool> {
    let address = request
        .headers()
        .get("cf-connecting-ip")?
        .unwrap_or_default();
    let hour = worker::Date::now().as_millis() / 3_600_000;
    let key = format!("rate:{address}:{hour}");
    let used: u32 = store
        .get(&key)
        .text()
        .await?
        .and_then(|text| text.parse().ok())
        .unwrap_or(0);
    if used >= HOURLY_LIMIT {
        return Ok(false);
    }
    store
        .put(&key, (used + 1).to_string())?
        .expiration_ttl(3600)
        .execute()
        .await?;
    Ok(true)
}

fn authorized(request: &Request, context: &RouteContext<()>) -> Result<bool> {
    let expected = context.secret("SYNC_TOKEN")?.to_string();
    let given = request.headers().get("authorization")?.unwrap_or_default();
    Ok(!expected.is_empty() && given == format!("Bearer {expected}"))
}

async fn submit(mut request: Request, context: RouteContext<()>) -> Result<Response> {
    let store = context.kv("REPORTS")?;
    if !within_limit(&store, &request).await? {
        return error("too many reports", 429);
    }
    let body = request.bytes().await?;
    if body.len() > MAX_BODY {
        return error("report too large", 413);
    }
    let Some(report) = serde_json::from_slice::<Value>(&body)
        .ok()
        .as_ref()
        .and_then(cleaned_report)
    else {
        return error("invalid report", 400);
    };
    let fingerprint = clipped(report.get("fingerprint"), 32);
    let version = clipped(report["environment"].get("astral"), 64);
    let existing: Option<Stored> = store.get(&report_key(&fingerprint)).json().await?;
    let mut stored = existing.unwrap_or(Stored {
        report: report.clone(),
        count: 0,
        versions: Vec::new(),
        issue: None,
    });
    stored.count += 1;
    let new_version = !version.is_empty() && !stored.versions.contains(&version);
    if new_version && stored.versions.len() < MAX_VERSIONS {
        stored.versions.push(version);
    }
    if stored.issue.is_none() || new_version {
        stored.report = report;
        store.put(&pending_key(&fingerprint), "")?.execute().await?;
    }
    store
        .put(&report_key(&fingerprint), serde_json::to_string(&stored)?)?
        .execute()
        .await?;
    Response::from_json(&json!({ "fingerprint": fingerprint, "issue": stored.issue }))
}

async fn lookup(_request: Request, context: RouteContext<()>) -> Result<Response> {
    let fingerprint = context.param("fingerprint").cloned().unwrap_or_default();
    if !valid_fingerprint(&fingerprint) {
        return error("invalid fingerprint", 400);
    }
    let stored: Option<Stored> = context
        .kv("REPORTS")?
        .get(&report_key(&fingerprint))
        .json()
        .await?;
    Response::from_json(
        &json!({ "fingerprint": fingerprint, "issue": stored.and_then(|stored| stored.issue) }),
    )
}

async fn pending(request: Request, context: RouteContext<()>) -> Result<Response> {
    if !authorized(&request, &context)? {
        return error("unauthorized", 401);
    }
    let store = context.kv("REPORTS")?;
    let listed = store
        .list()
        .prefix("pending:".into())
        .limit(PENDING_BATCH)
        .execute()
        .await?;
    let mut reports = Vec::new();
    for key in listed.keys {
        let fingerprint = key.name.trim_start_matches("pending:").to_owned();
        if let Some(stored) = store
            .get(&report_key(&fingerprint))
            .json::<Stored>()
            .await?
        {
            reports.push(json!({
                "fingerprint": fingerprint,
                "report": stored.report,
                "count": stored.count,
                "versions": stored.versions,
                "issue": stored.issue,
            }));
        } else {
            store.delete(&key.name).await?;
        }
    }
    Response::from_json(&json!({ "reports": reports }))
}

async fn synced(mut request: Request, context: RouteContext<()>) -> Result<Response> {
    if !authorized(&request, &context)? {
        return error("unauthorized", 401);
    }
    let Ok(synced) = request.json::<Synced>().await else {
        return error("invalid body", 400);
    };
    if !valid_fingerprint(&synced.fingerprint) || !synced.url.starts_with("https://github.com/") {
        return error("invalid body", 400);
    }
    let store = context.kv("REPORTS")?;
    let key = report_key(&synced.fingerprint);
    if let Some(mut stored) = store.get(&key).json::<Stored>().await? {
        stored.issue = Some(Issue {
            number: synced.number,
            url: synced.url,
        });
        store
            .put(&key, serde_json::to_string(&stored)?)?
            .execute()
            .await?;
    }
    store.delete(&pending_key(&synced.fingerprint)).await?;
    Response::from_json(&json!({ "ok": true }))
}

#[event(fetch)]
async fn fetch(request: Request, env: Env, _context: Context) -> Result<Response> {
    Router::new()
        .post_async("/v1/reports", submit)
        .get_async("/v1/reports/:fingerprint", lookup)
        .get_async("/v1/pending", pending)
        .post_async("/v1/synced", synced)
        .run(request, env)
        .await
}
