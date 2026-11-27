use std::time::Duration;

use serde_json::json;

use super::host::{self, Request};
use super::state::app;
use crate::lua::host as lua_host;
use crate::support::translations::{tr, tr_with};

pub const PROMPT: u64 = u64::MAX;
const SETTLE: Duration = Duration::from_millis(1500);

pub fn prompt() {
    std::thread::spawn(|| {
        std::thread::sleep(SETTLE);
        let spec = json!({
            "title": tr("ASTRAL_RECOVERY_TITLE"),
            "message": tr_with("ASTRAL_LAUNCH_RECOVERY", &[("count", crate::diagnostics::launch_guard::RECOVERY_AFTER.to_string())]),
            "image": "",
            "confirm": tr("ASTRAL_RECOVERY_RESUME"),
            "cancel": tr("ASTRAL_RECOVERY_KEEP_PAUSED"),
            "destructive": false,
        })
        .to_string();
        host::request(Request::Confirm {
            request: PROMPT,
            spec: &spec,
        });
    });
}

pub fn answered(resume: bool) {
    if !resume {
        return;
    }
    if app().is_some_and(|current| current.end_recovery()) {
        lua_host::resume();
        super::refresh_settings();
    }
}
