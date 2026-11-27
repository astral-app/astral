use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

use serde_json::Value;

use super::ffi::{raw, take, take_json};
use crate::plugins::packs::{self as packs, AstralPacksHandle};
use crate::settings::store::{self as settings, AstralSettingsHandle, Store};

const SETTINGS_RECHECK_MS: u64 = 500;
pub const DEFAULT_PACK: &str = "discord";
const SIGNING_KEY_FILE: &str = include_str!("../../../keys/plugin-signing.pub");

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Android,
    Ios,
}

impl Platform {
    pub const fn name(self) -> &'static str {
        match self {
            Platform::Android => "android",
            Platform::Ios => "ios",
        }
    }

    pub const fn image_name(self) -> &'static str {
        match self {
            Platform::Android => "libastral.so",
            Platform::Ios => "astral.dylib",
        }
    }

    pub const fn install_mode(self) -> &'static str {
        match self {
            Platform::Android => "injected",
            Platform::Ios => "package",
        }
    }
}

pub struct App {
    pub platform: Platform,
    pub version: String,
    pub commit: String,
    pub state: String,
    pub resources: String,
    pub discord_directory: String,
    pub discord_version: String,
    pub languages: Vec<String>,
    recovering: AtomicBool,
    pub environment: Mutex<Value>,
    settings: usize,
    packs: usize,
}

unsafe impl Send for App {}
unsafe impl Sync for App {}

static APP: OnceLock<App> = OnceLock::new();
static WIRE: Mutex<()> = Mutex::new(());
pub static IMAGE_GENERATION: AtomicU32 = AtomicU32::new(0);
pub static REFRESH_DECORATIONS: AtomicBool = AtomicBool::new(false);
pub static BLOCKED_REQUESTS: AtomicU64 = AtomicU64::new(0);

pub fn signing_key() -> &'static str {
    SIGNING_KEY_FILE.lines().next().unwrap_or_default().trim()
}

pub fn app() -> Option<&'static App> {
    APP.get()
}

pub type WireLock = unsafe extern "C" fn();

static EXTERNAL_WIRE_LOCK: OnceLock<(WireLock, WireLock)> = OnceLock::new();

pub fn share_wire_lock(lock: WireLock, unlock: WireLock) {
    let _ = EXTERNAL_WIRE_LOCK.set((lock, unlock));
}

pub enum WireGuard {
    Internal(MutexGuard<'static, ()>),
    External(WireLock),
}

impl Drop for WireGuard {
    fn drop(&mut self) {
        if let WireGuard::External(unlock) = self {
            unsafe { unlock() };
        }
    }
}

pub fn wire() -> WireGuard {
    if let Some((lock, unlock)) = EXTERNAL_WIRE_LOCK.get() {
        unsafe { lock() };
        return WireGuard::External(*unlock);
    }
    WireGuard::Internal(WIRE.lock().unwrap_or_else(|poisoned| poisoned.into_inner()))
}

pub struct Setup {
    pub platform: Platform,
    pub version: String,
    pub commit: String,
    pub state: String,
    pub resources: String,
    pub discord_directory: String,
    pub discord_version: String,
    pub languages: Vec<String>,
    pub recovering: bool,
    pub environment: Value,
}

pub fn install(setup: Setup) -> &'static App {
    APP.get_or_init(|| {
        let (path, length) = raw(&setup.state);
        let settings_path = format!("{}/settings.json", setup.state);
        let (settings_file, settings_length) = raw(&settings_path);
        let settings = unsafe { settings::astral_settings_open(settings_file, settings_length, SETTINGS_RECHECK_MS) };
        let (resources, resources_length) = raw(&setup.resources);
        let packs = unsafe { packs::astral_packs_open(settings, path, length, resources, resources_length) };
        App {
            platform: setup.platform,
            version: setup.version,
            commit: setup.commit,
            state: setup.state,
            resources: setup.resources,
            discord_directory: setup.discord_directory,
            discord_version: setup.discord_version,
            languages: setup.languages,
            recovering: AtomicBool::new(setup.recovering),
            environment: Mutex::new(setup.environment),
            settings: settings as usize,
            packs: packs as usize,
        }
    })
}

impl App {
    pub fn recovering(&self) -> bool {
        self.recovering.load(Ordering::Acquire)
    }

    pub fn end_recovery(&self) -> bool {
        self.recovering.swap(false, Ordering::AcqRel)
    }

    pub fn settings_handle(&self) -> *const AstralSettingsHandle {
        self.settings as *const AstralSettingsHandle
    }

    pub fn settings(&self) -> Option<MutexGuard<'static, Store>> {
        unsafe { settings::store(self.settings_handle()) }
    }

    pub fn flag(&self, store: &str, key: &str, fallback: bool) -> bool {
        self.settings()
            .map_or(fallback, |mut settings| settings.get_boolean(store, key, fallback))
    }

    pub fn text(&self, store: &str, key: &str, fallback: &str) -> String {
        self.settings()
            .and_then(|mut settings| settings.get_string(store, key))
            .unwrap_or_else(|| fallback.to_owned())
    }

    pub fn set(&self, store: &str, key: &str, value: Value) {
        if let Some(mut settings) = self.settings() {
            settings.set(store, key, value);
            settings.save();
        }
    }

    fn packs_handle(&self) -> *const AstralPacksHandle {
        self.packs as *const AstralPacksHandle
    }

    pub fn packs(&self, kind: PackKind) -> Value {
        take_json(unsafe { packs::astral_packs_list(self.packs_handle(), kind.code()) })
    }

    pub fn select_pack(&self, kind: PackKind, identifier: &str) -> bool {
        let (pointer, length) = raw(identifier);
        unsafe { packs::astral_packs_select(self.packs_handle(), kind.code(), pointer, length) != 0 }
    }

    pub fn font_faces(&self) -> Value {
        take_json(unsafe { packs::astral_packs_font_faces(self.packs_handle()) })
    }

    pub fn active_pack(&self, kind: PackKind) -> String {
        self.text("astral", kind.setting(), DEFAULT_PACK)
    }

    pub fn invalidate_packs(&self) {
        unsafe { packs::astral_packs_invalidate(self.packs_handle()) };
    }

    pub fn icon_path(&self, name: &str) -> String {
        let (pointer, length) = raw(name);
        take(unsafe { packs::astral_packs_icon_path(self.packs_handle(), pointer, length) })
    }

    pub fn emoji_url(&self, codepoints: &str) -> String {
        let (pointer, length) = raw(codepoints);
        take(unsafe { packs::astral_packs_emoji_url(self.packs_handle(), pointer, length) })
    }

    pub fn emoji_animated(&self) -> bool {
        self.flag("astral", "emoji-animated", true)
    }

    pub fn set_emoji_animated(&self, animated: bool) {
        self.set("astral", "emoji-animated", Value::Bool(animated));
    }

    pub fn blocked_requests(&self) -> u64 {
        BLOCKED_REQUESTS.load(Ordering::Relaxed)
    }
}

#[derive(Clone, Copy)]
pub enum PackKind {
    Icons,
    Emoji,
    Fonts,
}

impl PackKind {
    const fn code(self) -> core::ffi::c_int {
        match self {
            PackKind::Icons => 0,
            PackKind::Emoji => 1,
            PackKind::Fonts => 2,
        }
    }

    const fn setting(self) -> &'static str {
        match self {
            PackKind::Icons => "icon-pack",
            PackKind::Emoji => "emoji-pack",
            PackKind::Fonts => "font-pack",
        }
    }
}
