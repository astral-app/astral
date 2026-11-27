use std::sync::Mutex;

use serde_json::json;

use super::host::{self, Request};
use crate::settings::pages::{install_linked, link_package, linked_installed};
use crate::support::translations::{tr, tr_with};

pub const PROMPT: u64 = u64::MAX - 1;
const SCHEME: &str = "astral://";
const KINDS: [&str; 5] = ["plugin", "theme", "icon-pack", "emoji-pack", "font-pack"];

static PENDING: Mutex<Option<(String, String)>> = Mutex::new(None);
static EARLY: Mutex<Option<String>> = Mutex::new(None);

pub fn install_target(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix(SCHEME)?;
    let rest = rest.split(['?', '#']).next().unwrap_or_default();
    let mut parts = rest.trim_matches('/').split('/');
    let (Some("install"), Some(kind), Some(identifier), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
        return None;
    };
    let valid = !identifier.is_empty()
        && identifier
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'));
    (KINDS.contains(&kind) && valid).then(|| (kind.to_owned(), identifier.to_owned()))
}

pub fn open(url: &str) -> bool {
    if install_target(url).is_none() {
        return false;
    }
    if !super::started() {
        *EARLY.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(url.to_owned());
        return true;
    }
    prompt(url);
    true
}

pub fn open_early() {
    let early = EARLY.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take();
    if let Some(url) = early {
        prompt(&url);
    }
}

fn prompt(url: &str) {
    let Some((kind, identifier)) = install_target(url) else {
        return;
    };
    let Some(package) = link_package(&kind, &identifier) else {
        host::request(Request::ShowToast {
            title: tr("ASTRAL_LINK_UNKNOWN_ADDON"),
            message: &identifier,
        });
        return;
    };
    *PENDING.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some((kind, identifier));
    let details = [
        json!({"icon": "res://ic_mana_shop", "text": tr("ASTRAL_LINK_FROM_MARKETPLACE")}),
        json!({"icon": "res://ic_mana_shield", "text": tr("ASTRAL_LINK_SIGNED")}),
    ];
    let spec = json!({
        "title": tr_with("ASTRAL_LINK_INSTALL_TITLE", &[("name", package.name)]),
        "message": package.description,
        "icon": package.icon,
        "subtitle": package.byline,
        "details": details,
        "image": "",
        "confirm": tr("ASTRAL_LINK_INSTALL"),
        "cancel": tr("ASTRAL_LINK_CANCEL"),
        "destructive": false,
    })
    .to_string();
    host::request(Request::Confirm {
        request: PROMPT,
        spec: &spec,
    });
}

pub fn answered(install: bool) {
    let pending = PENDING.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take();
    if let Some((kind, identifier)) = pending.filter(|_| install) {
        std::thread::spawn(move || {
            let name = link_package(&kind, &identifier).map(|package| package.name).unwrap_or_default();
            let (installed_kind, installed_identifier) = (kind.clone(), identifier.clone());
            let done: crate::support::host_callback::Changed = std::sync::Arc::new(move || {
                let key = if linked_installed(&installed_kind, &installed_identifier) {
                    "ASTRAL_LINK_INSTALLED"
                } else {
                    "ASTRAL_LINK_INSTALL_FAILED"
                };
                host::request(Request::ShowToast {
                    title: &tr_with(key, &[("name", name.clone())]),
                    message: "",
                });
            });
            install_linked(&kind, &identifier, done);
        });
    }
}
