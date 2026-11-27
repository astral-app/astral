use core::ffi::{c_char, c_int};
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use crate::diagnostics::exit_record::EXIT_ENDED;
use crate::support::bridge::{AstralRustString, path};
use crate::support::files::{read_text, remove, write_atomically};
use crate::support::log::{ERROR, INFO, WARNING, log};
use crate::support::translations::tr_with;

const FILE: &str = "launch-guard.json";
const ROLL_BACK_AFTER: c_int = 2;
pub const RECOVERY_AFTER: c_int = 3;
const CATEGORY: &str = "default";

#[repr(C)]
#[derive(Default)]
pub struct AstralLaunchVerdict {
    pub failures: c_int,
    pub roll_back: c_int,
    pub enter_recovery: c_int,
}

struct Marker {
    failures: i64,
    frozen: bool,
    shown: bool,
}

fn guard_path(directory: &Path) -> PathBuf {
    directory.join(FILE)
}

fn read_marker(contents: Option<Vec<u8>>) -> Marker {
    let Some(contents) = contents else {
        return Marker {
            failures: -1,
            frozen: false,
            shown: true,
        };
    };
    let parsed: Value = serde_json::from_slice(&contents).unwrap_or(Value::Null);
    let object = parsed.as_object();
    let field = |name: &str| object.and_then(|object| object.get(name));
    Marker {
        failures: field("failures")
            .and_then(Value::as_i64)
            .filter(|failures| *failures >= 0)
            .unwrap_or(0),
        frozen: field("frozen").and_then(Value::as_bool).unwrap_or(false),
        shown: field("shown").and_then(Value::as_bool).unwrap_or(true),
    }
}

fn marker_json(marker: &Marker) -> Vec<u8> {
    let mut object = Map::new();
    object.insert("failures".into(), json!(marker.failures));
    if marker.frozen {
        object.insert("frozen".into(), json!(true));
    }
    if !marker.shown {
        object.insert("shown".into(), json!(false));
    }
    Value::Object(object).to_string().into_bytes()
}

fn arm(directory: &Path, marker: &Marker) {
    let _ = std::fs::create_dir_all(directory);
    if !write_atomically(&guard_path(directory), &marker_json(marker)) {
        log(ERROR, CATEGORY, "Failed to arm the launch guard.");
    }
}

fn plain(failures: i64) -> Marker {
    Marker {
        failures,
        frozen: false,
        shown: true,
    }
}

pub fn begin(directory: &Path, previous: c_int) -> AstralLaunchVerdict {
    let marker = read_marker(read_text(&guard_path(directory)));
    let mut recorded = marker.failures;
    if recorded >= 0 && !marker.shown && !marker.frozen {
        log(
            INFO,
            CATEGORY,
            "Discord's previous launch ran only in the background and never showed; it does not count.",
        );
        arm(directory, &plain(recorded));
        return AstralLaunchVerdict {
            failures: recorded as c_int,
            ..AstralLaunchVerdict::default()
        };
    }
    if recorded >= 0 && marker.frozen {
        log(WARNING, CATEGORY, "Discord's previous launch never rendered its first screen.");
    } else if recorded >= 0 && previous == EXIT_ENDED {
        log(
            INFO,
            CATEGORY,
            "Discord's previous launch was closed before it was healthy, without crashing.",
        );
        recorded = -1;
    }
    let failures = (recorded + 1) as c_int;
    if failures > 0 {
        log(
            WARNING,
            CATEGORY,
            &format!("Discord's previous launch ended before it was healthy; {failures} in a row."),
        );
    }
    arm(directory, &plain(i64::from(failures)));
    AstralLaunchVerdict {
        failures,
        roll_back: c_int::from(failures == ROLL_BACK_AFTER),
        enter_recovery: c_int::from(failures == RECOVERY_AFTER),
    }
}

fn update(directory: &Path, change: impl FnOnce(&mut Marker) -> bool) -> bool {
    let file = guard_path(directory);
    let mut marker = read_marker(read_text(&file));
    if marker.failures < 0 {
        return false;
    }
    if !change(&mut marker) {
        return true;
    }
    write_atomically(&file, &marker_json(&marker))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_launch_guard_begin(directory: *const c_char, length: usize, previous: c_int) -> AstralLaunchVerdict {
    begin(path(directory, length), previous)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_launch_guard_succeed(directory: *const c_char, length: usize) -> c_int {
    c_int::from(remove(&guard_path(path(directory, length))))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_launch_guard_mark_frozen(directory: *const c_char, length: usize) -> c_int {
    c_int::from(update(path(directory, length), |marker| {
        marker.frozen = true;
        true
    }))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_launch_guard_mark_hidden(directory: *const c_char, length: usize) -> c_int {
    c_int::from(update(path(directory, length), |marker| {
        marker.shown = false;
        true
    }))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_launch_guard_mark_shown(directory: *const c_char, length: usize) -> c_int {
    c_int::from(update(path(directory, length), |marker| {
        let hidden = !marker.shown;
        marker.shown = true;
        hidden
    }))
}

fn notice(key: &'static str, failures: c_int) -> AstralRustString {
    AstralRustString::new(tr_with(key, &[("count", failures.to_string())]).into_bytes())
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_launch_roll_back_notice(failures: c_int) -> AstralRustString {
    notice("ASTRAL_LAUNCH_ROLLED_BACK", failures)
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_launch_recovery_notice(failures: c_int) -> AstralRustString {
    notice("ASTRAL_LAUNCH_RECOVERY", failures)
}
