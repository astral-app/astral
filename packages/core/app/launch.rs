use std::io::Read;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value, json};

const STATUS_SCHEMA: u64 = 2;
const STATUS_FILE: &str = "status.json";
const READINESS_ATTEMPTS: u32 = 24;
const READY: &str = "ready";
const CHECKING: &str = "checking";
const RETRYABLE: [&str; 2] = ["phoenix-image-unavailable", "phoenix-root-inactive"];
const RUN_ID_BYTES: usize = 16;
const RANDOM_SOURCE: &str = "/dev/urandom";
const PLUGIN_SUMMARY_KEYS: [&str; 5] = ["id", "enabled", "running", "failed", "error"];

pub const READINESS_START: i32 = 0;
pub const READINESS_RETRY: i32 = 1;
pub const READINESS_WAIT: i32 = 2;

#[derive(Default)]
struct Launch {
    run_id: String,
    stage: String,
    compatibility: String,
    started: bool,
    attempts: u32,
    retry_scheduled: bool,
}

static LAUNCH: Mutex<Option<Launch>> = Mutex::new(None);

fn with_launch<T>(work: impl FnOnce(&mut Launch) -> T) -> Option<T> {
    let mut guard = LAUNCH.lock().ok()?;
    Some(work(guard.get_or_insert_with(|| Launch {
        run_id: run_id(),
        ..Launch::default()
    })))
}

fn run_id() -> String {
    let mut bytes = [0u8; RUN_ID_BYTES];
    if let Ok(mut random) = std::fs::File::open(RANDOM_SOURCE) {
        let _ = random.read_exact(&mut bytes);
    }
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn plugin_summaries() -> Value {
    let plugins: Value = serde_json::from_str(&super::plugins()).unwrap_or(Value::Null);
    let summaries = plugins
        .as_array()
        .into_iter()
        .flatten()
        .map(|plugin| {
            let summary: Map<String, Value> = PLUGIN_SUMMARY_KEYS
                .iter()
                .filter_map(|key| plugin.get(*key).map(|value| ((*key).to_owned(), value.clone())))
                .collect();
            Value::Object(summary)
        })
        .collect();
    Value::Array(summaries)
}

pub fn status_report(stage: &str, facts: &Value) -> Value {
    let (run_id, compatibility) = with_launch(|launch| {
        launch.stage = stage.to_owned();
        (launch.run_id.clone(), launch.compatibility.clone())
    })
    .unwrap_or_default();
    let mut report = Map::new();
    if let Some(facts) = facts.as_object() {
        report.extend(facts.clone());
    }
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |elapsed| elapsed.as_secs_f64());
    for (key, value) in [
        ("schema", json!(STATUS_SCHEMA)),
        ("runID", json!(run_id)),
        ("pid", json!(std::process::id())),
        ("stage", json!(stage)),
        (
            "compatibility",
            json!(if compatibility.is_empty() {
                CHECKING
            } else {
                compatibility.as_str()
            }),
        ),
        ("recovering", json!(super::state::app().is_some_and(|current| current.recovering()))),
        ("time", json!(time)),
        ("plugins", plugin_summaries()),
    ] {
        report.insert(key.to_owned(), value);
    }
    Value::Object(report)
}

pub fn write_status(stage: &str, facts: &str) {
    let report = status_report(stage, &serde_json::from_str(facts).unwrap_or(Value::Null));
    let Some(directory) = super::state::app().map(|current| current.state.clone()) else {
        return;
    };
    let path = std::path::Path::new(&directory).join(STATUS_FILE);
    if !crate::support::files::write_atomically(&path, report.to_string().as_bytes()) {
        super::host::log(&format!("Failed to write status.json {}", path.display()));
    }
}

pub fn last_stage() -> String {
    with_launch(|launch| launch.stage.clone()).unwrap_or_default()
}

pub fn claim_start() -> bool {
    with_launch(|launch| !std::mem::replace(&mut launch.started, true)).unwrap_or(false)
}

pub fn started() -> bool {
    with_launch(|launch| launch.started).unwrap_or(false)
}

pub fn foregrounded() -> bool {
    with_launch(|launch| {
        if launch.retry_scheduled {
            return false;
        }
        launch.attempts = 0;
        true
    })
    .unwrap_or(false)
}

pub struct Readiness {
    pub action: i32,
    pub report: bool,
}

pub fn check_readiness(compatibility: &str) -> Readiness {
    with_launch(|launch| {
        launch.retry_scheduled = false;
        launch.attempts += 1;
        launch.compatibility = compatibility.to_owned();
        if compatibility == READY {
            return Readiness {
                action: READINESS_START,
                report: false,
            };
        }
        let report = launch.attempts == 1 || launch.attempts == READINESS_ATTEMPTS;
        let retry = launch.attempts < READINESS_ATTEMPTS && RETRYABLE.contains(&compatibility);
        launch.retry_scheduled = retry;
        Readiness {
            action: if retry { READINESS_RETRY } else { READINESS_WAIT },
            report,
        }
    })
    .unwrap_or(Readiness {
        action: READINESS_WAIT,
        report: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reset() {
        *LAUNCH.lock().unwrap() = Some(Launch {
            run_id: run_id(),
            ..Launch::default()
        });
    }

    #[test]
    fn readiness_retries_only_retryable_states_and_reports_ends() {
        reset();
        let first = check_readiness("phoenix-root-inactive");
        assert_eq!((first.action, first.report), (READINESS_RETRY, true));
        assert!(!foregrounded());
        for _ in 2..READINESS_ATTEMPTS {
            assert!(!check_readiness("phoenix-root-inactive").report);
        }
        let last = check_readiness("phoenix-root-inactive");
        assert_eq!((last.action, last.report), (READINESS_WAIT, true));
        assert!(foregrounded());
        assert_eq!(check_readiness("unsupported").action, READINESS_WAIT);
        assert_eq!(check_readiness(READY).action, READINESS_START);
    }

    #[test]
    fn starts_once_and_reports_the_run() {
        reset();
        assert!(claim_start());
        assert!(!claim_start());
        assert!(started());
        let report = status_report("ready", &json!({ "route": "home" }));
        assert_eq!(report["stage"], "ready");
        assert_eq!(report["route"], "home");
        assert_eq!(report["runID"].as_str().unwrap().len(), RUN_ID_BYTES * 2);
        assert_eq!(last_stage(), "ready");
    }
}
