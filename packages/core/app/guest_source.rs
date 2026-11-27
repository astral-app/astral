use std::sync::Mutex;

use super::host::{self, Request};
use crate::support::translations::tr;

#[derive(Clone, Default)]
pub struct GuestSource {
    pub bundled: bool,
    pub source: String,
}

static CURRENT: Mutex<Option<GuestSource>> = Mutex::new(None);

pub fn current() -> Option<GuestSource> {
    CURRENT.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
}

pub fn note(bundled: bool, source: &str) {
    let first = {
        let mut current = CURRENT.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let first = current.as_ref().is_none_or(|known| known.bundled);
        *current = Some(GuestSource {
            bundled,
            source: source.to_owned(),
        });
        first
    };
    if bundled || !first {
        return;
    }
    host::log(&format!(
        "Phoenix runs app code from an over-the-air update ({source}); astral was checked against the built-in code"
    ));
    std::thread::spawn(|| {
        host::request(Request::ShowAlert {
            title: tr("ASTRAL_OTA_GUEST_TITLE"),
            message: tr("ASTRAL_OTA_GUEST_MESSAGE"),
        });
    });
}
