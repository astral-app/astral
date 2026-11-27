use core::ffi::{c_char, c_int, c_void};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use serde_json::{Map, Value};

use crate::plugins::signatures::{signed_files, verify, verify_directory};
use crate::settings::store::{AstralSettingsHandle, store as settings_store};
use crate::support::bridge::{AstralRustString, bytes, optional, path};
use crate::support::crypto::{hex, sha256};
use crate::support::files::{read_text, resolve_inside, write_atomically};
use crate::support::log::{ERROR, log};
use crate::support::toml_json;

const CATALOG_LIMIT: usize = 512 << 10;
const PUBLISHED_CATALOG: &str = "https://astral-app.github.io/astral/catalog/index.toml";
const SIGNATURE_LIMIT: usize = 512;
const PLUGIN_SIGNATURE_LIMIT: usize = 128 << 10;
const FILE_LIMIT: usize = 1 << 20;
const PLUGIN_LIMIT: usize = 2 << 20;
const PACK_LIMIT: usize = 16 << 20;
const DOWNLOAD_WORKERS: usize = 8;
const INSTALLING: &str = "Installing…";
const FAILED: &str = "Failed";
pub const KIND_PLUGIN: c_int = 0;
pub const KIND_THEME: c_int = 1;
pub const KIND_ICON_PACK: c_int = 2;
pub const KIND_EMOJI_PACK: c_int = 3;

pub type Fetch = unsafe extern "C" fn(*mut c_void, *const c_char, usize, *mut AstralRustString) -> c_int;
pub type Installed = unsafe extern "C" fn(*mut c_void, c_int, *const c_char, usize);
pub type Changed = unsafe extern "C" fn(*mut c_void);
pub type Release = unsafe extern "C" fn(*mut c_void);

struct Config {
    settings: usize,
    state: PathBuf,
    resources: PathBuf,
    key: Vec<u8>,
    fetch: Fetch,
    fetch_context: usize,
    installed: Option<Installed>,
    installed_context: usize,
}

struct Notify {
    call: Option<Changed>,
    context: usize,
    release: Option<Release>,
}

impl Notify {
    fn changed(&self) {
        if let Some(call) = self.call {
            unsafe { call(self.context as *mut c_void) };
        }
    }
}

impl Drop for Notify {
    fn drop(&mut self) {
        if let Some(release) = self.release {
            unsafe { release(self.context as *mut c_void) };
        }
    }
}

struct Catalog {
    loading: bool,
    state: &'static str,
    error: String,
    entries: Value,
    installs: HashMap<String, &'static str>,
}

static CONFIG: Mutex<Option<Arc<Config>>> = Mutex::new(None);
static CATALOG: Mutex<Option<Catalog>> = Mutex::new(None);

fn config() -> Option<Arc<Config>> {
    CONFIG.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
}

fn catalog_state() -> MutexGuard<'static, Option<Catalog>> {
    let mut guard = CATALOG.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.get_or_insert_with(|| Catalog {
        loading: false,
        state: "idle",
        error: String::new(),
        entries: Value::Object(Map::new()),
        installs: HashMap::new(),
    });
    guard
}

fn with_catalog<T>(work: impl FnOnce(&mut Catalog) -> T) -> T {
    let mut guard = catalog_state();
    work(guard.as_mut().expect("catalog state initialised"))
}

pub fn current_catalog() -> Value {
    with_catalog(|catalog| catalog.entries.clone())
}

pub fn valid_package_identifier(identifier: &str) -> bool {
    !identifier.is_empty()
        && identifier.len() <= 128
        && !identifier.contains("..")
        && identifier
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

pub fn valid_plugin_identifier(identifier: &str) -> bool {
    (3..=80).contains(&identifier.len())
        && !identifier.contains("..")
        && identifier
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-'))
}

fn safe_relative(relative: &str) -> bool {
    !relative.is_empty()
        && relative.len() <= 512
        && !relative.contains(['\\', '\0'])
        && relative
            .strip_suffix('/')
            .unwrap_or(relative)
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn https_authority_valid(url: &str) -> bool {
    let rest = &url[8..];
    let authority = &rest[..rest.find(['/', '?', '#']).unwrap_or(rest.len())];
    !authority.is_empty() && !authority.contains('@')
}

struct SignedPackage {
    identifier: String,
    base: String,
    signature: Vec<u8>,
    files: Vec<(String, String)>,
}

impl Config {
    fn catalog_url(&self) -> String {
        unsafe { settings_store(self.settings as *const AstralSettingsHandle) }
            .and_then(|mut store| store.get_string("astral", "catalog"))
            .unwrap_or_else(|| PUBLISHED_CATALOG.to_owned())
    }

    fn bundled_catalog(&self) -> Option<Value> {
        let bundled = self.resources.join("catalog/index.toml");
        self.verified_file(&bundled, &bundled.with_extension("toml.sig"))
    }

    fn fetch_content(&self, url: &str, maximum: usize) -> Result<Vec<u8>, String> {
        if let Some(file) = url.strip_prefix("file://") {
            let contents = std::fs::metadata(file)
                .ok()
                .filter(|metadata| metadata.len() <= maximum as u64)
                .and_then(|_| read_text(Path::new(file)))
                .filter(|contents| contents.len() <= maximum);
            return contents.ok_or_else(|| format!("could not read {file}"));
        }
        if !url.starts_with("https://") {
            return Err("downloads must use https".to_owned());
        }
        if !https_authority_valid(url) || url.contains('\\') {
            return Err("invalid https URL".to_owned());
        }
        let mut body = AstralRustString {
            bytes: core::ptr::null_mut(),
            length: 0,
        };
        let status = unsafe { (self.fetch)(self.fetch_context as *mut c_void, url.as_ptr().cast(), url.len(), &mut body) };
        let data = bytes(body.bytes.cast(), body.length).to_vec();
        unsafe { crate::support::bridge::astral_rust_string_free(body) };
        match i64::from(status) {
            crate::network::status::OK if data.len() > maximum => Err("download too large".to_owned()),
            crate::network::status::OK => Ok(data),
            0 => Err("request failed".to_owned()),
            status => Err(format!("HTTP {status}")),
        }
    }

    fn resolve_reference(&self, reference: &str) -> Option<String> {
        if reference.is_empty() || reference.len() > 4096 || reference.contains(['\\', '\0']) {
            return None;
        }
        if reference.contains("://") {
            return Some(reference.to_owned());
        }
        if !safe_relative(reference) {
            return None;
        }
        let source = self.catalog_url();
        if let Some(file) = source.strip_prefix("file://") {
            let parent = Path::new(file).parent().unwrap_or(Path::new("/"));
            let resolved = parent.join(reference);
            if resolved.components().any(|part| part == Component::ParentDir) {
                return None;
            }
            return Some(format!("file://{}", resolved.display()));
        }
        if !source.starts_with("https://") || !https_authority_valid(&source) {
            return None;
        }
        let directory = match source.rfind('/') {
            Some(slash) if slash >= 8 => &source[..=slash],
            _ => return Some(format!("{source}/{reference}")),
        };
        Some(format!("{directory}{reference}"))
    }

    fn fetch_package_path(&self, base: &str, relative: &str, maximum: usize) -> Result<Vec<u8>, String> {
        let mut url = self.resolve_reference(base).ok_or_else(|| "invalid package URL".to_owned())?;
        if !relative.is_empty() {
            if !url.ends_with('/') {
                url.push('/');
            }
            url.push_str(relative);
        }
        self.fetch_content(&url, maximum)
    }

    fn parse_verified(&self, contents: &[u8], signature: &[u8]) -> Option<Value> {
        if !verify(&self.key, contents, signature) {
            return None;
        }
        let parsed = toml_json::parse(std::str::from_utf8(contents).ok()?)?;
        let section = |name: &str| {
            parsed
                .get(name)
                .filter(|value| value.is_object())
                .cloned()
                .unwrap_or_else(|| Value::Object(Map::new()))
        };
        let entries = |name: &str| {
            Value::Array(
                parsed
                    .get(name)
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter(|entry| {
                        ["id", "name", "base"]
                            .iter()
                            .all(|field| entry.get(field).is_some_and(Value::is_string))
                    })
                    .cloned()
                    .collect(),
            )
        };
        let mut result = Map::new();
        result.insert("astral".into(), section("astral"));
        result.insert("compatibility".into(), section("compatibility"));
        result.insert("plugins".into(), entries("plugins"));
        result.insert("themes".into(), entries("themes"));
        result.insert("icon-packs".into(), entries("icon-packs"));
        result.insert("emoji-packs".into(), entries("emoji-packs"));
        Some(Value::Object(result))
    }

    fn cache(&self) -> (PathBuf, PathBuf) {
        let directory = self.state.join("catalog");
        (directory.join("index.toml"), directory.join("index.toml.sig"))
    }

    fn download_catalog(&self, url: &str) -> Result<Value, String> {
        let contents = self.fetch_content(url, CATALOG_LIMIT)?;
        let signature = self.fetch_content(&format!("{url}.sig"), SIGNATURE_LIMIT)?;
        let catalog = self
            .parse_verified(&contents, &signature)
            .ok_or_else(|| "catalog signature or TOML is invalid".to_owned())?;
        if url.starts_with("https://") {
            let (index, sig) = self.cache();
            write_atomically(&index, &contents);
            write_atomically(&sig, &signature);
        }
        Ok(catalog)
    }

    fn verified_file(&self, index: &Path, sig: &Path) -> Option<Value> {
        self.parse_verified(&read_text(index)?, &read_text(sig)?)
    }

    fn cached_catalog(&self) -> Option<Value> {
        let (index, sig) = self.cache();
        self.verified_file(&index, &sig)
    }

    fn stop_list_reason(&self, phoenix: &str) -> Option<String> {
        if phoenix.is_empty() {
            return None;
        }
        let catalog = self.cached_catalog().or_else(|| self.bundled_catalog())?;
        let passive = catalog.get("compatibility")?.get("passive")?.as_array()?;
        passive.iter().find_map(|entry| {
            let uuid = entry.as_str().or_else(|| entry.get("phoenix").and_then(Value::as_str))?;
            if !uuid.eq_ignore_ascii_case(phoenix) {
                return None;
            }
            let reason = entry.get("reason").and_then(Value::as_str).filter(|reason| !reason.is_empty());
            Some(reason.unwrap_or("listed as passive in the catalog").to_owned())
        })
    }

    fn install_theme(&self, package: &Value) -> Result<(), String> {
        let identifier = package.get("id").and_then(Value::as_str).unwrap_or("");
        let base = package.get("base").and_then(Value::as_str).unwrap_or("");
        if !valid_package_identifier(identifier) || base.is_empty() {
            return Err("invalid theme package".to_owned());
        }
        let contents = self.fetch_package_path(base, "", FILE_LIMIT)?;
        let theme = std::str::from_utf8(&contents).ok().and_then(toml_json::parse);
        let matches = theme.as_ref().is_some_and(|theme| {
            theme.get("id").and_then(Value::as_str) == Some(identifier) && theme.get("colors").is_some_and(Value::is_object)
        });
        if !matches {
            return Err("theme does not match the catalog".to_owned());
        }
        let directory = self.state.join("themes");
        let _ = std::fs::create_dir_all(&directory);
        if !write_atomically(&directory.join(format!("{identifier}.toml")), &contents) {
            return Err("could not write theme".to_owned());
        }
        Ok(())
    }

    fn stage_files(&self, base: &str, staging: &Path, signature: &[u8], files: &[(String, String)], limit: usize) -> Result<(), String> {
        std::fs::create_dir_all(staging).map_err(|_| "could not prepare package files".to_owned())?;
        if !write_atomically(&staging.join("signature"), signature) {
            return Err("could not prepare package files".to_owned());
        }
        let total = AtomicUsize::new(0);
        let next = AtomicUsize::new(0);
        let failure: Mutex<Option<String>> = Mutex::new(None);
        std::thread::scope(|scope| {
            for _ in 0..DOWNLOAD_WORKERS.min(files.len()) {
                scope.spawn(|| {
                    while let Some((relative, digest)) = files.get(next.fetch_add(1, Ordering::Relaxed)) {
                        let mut failed = failure.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                        if failed.is_some() {
                            return;
                        }
                        drop(failed);
                        if let Err(error) = self.stage_file(base, staging, relative, digest, &total, limit) {
                            failed = failure.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                            *failed = Some(error);
                            return;
                        }
                    }
                });
            }
        });
        failure
            .into_inner()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .map_or(Ok(()), Err)
    }

    fn stage_file(
        &self,
        base: &str,
        staging: &Path,
        relative: &str,
        digest: &str,
        total: &AtomicUsize,
        limit: usize,
    ) -> Result<(), String> {
        let contents = self.fetch_package_path(base, relative, FILE_LIMIT)?;
        if total.fetch_add(contents.len(), Ordering::Relaxed) + contents.len() > limit {
            return Err("package download exceeds the size limit".to_owned());
        }
        if hex(&sha256(&contents)) != digest {
            return Err("downloaded files do not match their signature".to_owned());
        }
        let staging_text = staging.to_string_lossy();
        let destination =
            resolve_inside(staging_text.as_bytes(), relative.as_bytes()).ok_or_else(|| "could not write package files".to_owned())?;
        let destination = path(destination.as_ptr().cast(), destination.len()).to_path_buf();
        let written =
            destination.parent().is_some_and(|parent| std::fs::create_dir_all(parent).is_ok()) && write_atomically(&destination, &contents);
        if !written {
            return Err("could not write package files".to_owned());
        }
        Ok(())
    }

    fn replace_installed(root: &Path, identifier: &str, staging: &Path) -> Result<(), String> {
        let target = root.join(identifier);
        let previous = root.join(format!(".{identifier}.previous"));
        let _ = std::fs::remove_dir_all(&previous);
        if target.exists() && std::fs::rename(&target, &previous).is_err() {
            let _ = std::fs::remove_dir_all(staging);
            return Err("could not replace the installed package".to_owned());
        }
        if std::fs::rename(staging, &target).is_err() {
            if previous.exists() {
                let _ = std::fs::rename(&previous, &target);
            }
            let _ = std::fs::remove_dir_all(staging);
            return Err("could not install the package".to_owned());
        }
        let _ = std::fs::remove_dir_all(&previous);
        Ok(())
    }

    fn signed_package(&self, package: &Value) -> Result<SignedPackage, String> {
        let identifier = package.get("id").and_then(Value::as_str).unwrap_or("");
        let base = package.get("base").and_then(Value::as_str).unwrap_or("");
        if !valid_package_identifier(identifier) || base.is_empty() {
            return Err("invalid package".to_owned());
        }
        let signature = self.fetch_package_path(base, "signature", PLUGIN_SIGNATURE_LIMIT)?;
        let files: Vec<(String, String)> = signed_files(&self.key, &signature)
            .ok_or_else(|| "package signature is invalid".to_owned())?
            .into_iter()
            .collect();
        if !files.iter().any(|(relative, _)| relative == "manifest.toml") {
            return Err("package signature lists no manifest".to_owned());
        }
        Ok(SignedPackage {
            identifier: identifier.to_owned(),
            base: base.to_owned(),
            signature,
            files,
        })
    }

    fn install_pack(&self, package: &Value, folder: &str) -> Result<String, String> {
        let SignedPackage {
            identifier,
            base,
            signature,
            files,
        } = self.signed_package(package)?;
        let root = self.state.join(folder);
        let staging = root.join(format!(".{identifier}.installing"));
        let _ = std::fs::remove_dir_all(&staging);
        if let Err(failure) = self.stage_files(&base, &staging, &signature, &files, PACK_LIMIT) {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(failure);
        }
        let matches = read_text(&staging.join("manifest.toml"))
            .and_then(|text| String::from_utf8(text).ok())
            .and_then(|text| toml_json::parse(&text))
            .is_some_and(|manifest| manifest.get("id").and_then(Value::as_str) == Some(identifier.as_str()));
        if !matches {
            let _ = std::fs::remove_dir_all(&staging);
            return Err("pack does not match the catalog".to_owned());
        }
        Self::replace_installed(&root, &identifier, &staging)?;
        Ok(identifier)
    }

    fn install_plugin(&self, package: &Value) -> Result<String, String> {
        let SignedPackage {
            identifier,
            base,
            signature,
            files,
        } = self.signed_package(package)?;
        if !valid_plugin_identifier(&identifier) {
            return Err("invalid plugin package".to_owned());
        }
        let identifier = identifier.as_str();
        let root = self.state.join("plugins");
        let staging = root.join(format!(".{identifier}.installing"));
        let _ = std::fs::remove_dir_all(&staging);
        if let Err(failure) = self.stage_files(&base, &staging, &signature, &files, PLUGIN_LIMIT) {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(failure);
        }
        let manifest = read_text(&staging.join("manifest.toml"))
            .and_then(|text| String::from_utf8(text).ok())
            .and_then(|text| toml_json::parse(&text));
        let main = manifest
            .as_ref()
            .map(|manifest| manifest.get("main").and_then(Value::as_str).unwrap_or("main.lua").to_owned())
            .unwrap_or_default();
        let staging_text = staging.to_string_lossy().into_owned();
        let valid = manifest
            .as_ref()
            .is_some_and(|manifest| manifest.get("id").and_then(Value::as_str) == Some(identifier))
            && !main.contains('/')
            && files.iter().any(|(relative, _)| *relative == main)
            && verify_directory(&self.key, staging_text.as_bytes(), &main, |contents| {
                hex(&sha256(contents)).into_bytes()
            });
        if !valid {
            let _ = std::fs::remove_dir_all(&staging);
            return Err("downloaded plugin files do not match their signature".to_owned());
        }
        Self::replace_installed(&root, identifier, &staging)?;
        Ok(identifier.to_owned())
    }
}

fn refresh(notify: Notify) {
    let Some(config) = config() else { return };
    let started = with_catalog(|catalog| {
        if catalog.loading {
            return false;
        }
        catalog.loading = true;
        catalog.state = "loading";
        catalog.error.clear();
        true
    });
    if !started {
        return;
    }
    notify.changed();
    std::thread::spawn(move || {
        let url = config.catalog_url();
        let downloaded = config.download_catalog(&url);
        let failure = downloaded.as_ref().err().cloned().unwrap_or_default();
        let result = downloaded
            .ok()
            .or_else(|| config.cached_catalog())
            .or_else(|| config.bundled_catalog());
        if !failure.is_empty() {
            log(ERROR, "marketplace", &format!("Failed to load catalog: {failure}"));
        }
        with_catalog(|catalog| {
            if let Some(result) = result {
                catalog.entries = result;
            }
            let empty = catalog.entries.as_object().is_none_or(Map::is_empty);
            catalog.state = if empty { "failed" } else { "ready" };
            catalog.error = failure;
            catalog.loading = false;
        });
        notify.changed();
    });
}

fn install(package: Value, kind: c_int, notify: Notify) {
    let Some(config) = config() else { return };
    let identifier = package.get("id").and_then(Value::as_str).unwrap_or("").to_owned();
    if !valid_package_identifier(&identifier) || !(KIND_PLUGIN..=KIND_EMOJI_PACK).contains(&kind) {
        return;
    }
    let claimed = with_catalog(|catalog| {
        if catalog.installs.get(&identifier) == Some(&INSTALLING) {
            return false;
        }
        catalog.installs.insert(identifier.clone(), INSTALLING);
        true
    });
    if !claimed {
        return;
    }
    notify.changed();
    std::thread::spawn(move || {
        let result = match kind {
            KIND_THEME => config.install_theme(&package).map(|()| identifier.clone()),
            KIND_ICON_PACK => config.install_pack(&package, "icon-packs"),
            KIND_EMOJI_PACK => config.install_pack(&package, "emoji-packs"),
            _ => config.install_plugin(&package),
        };
        with_catalog(|catalog| match &result {
            Ok(_) => {
                catalog.installs.remove(&identifier);
            }
            Err(_) => {
                catalog.installs.insert(identifier.clone(), FAILED);
            }
        });
        match result {
            Ok(installed) => {
                if let Some(callback) = config.installed {
                    unsafe {
                        callback(
                            config.installed_context as *mut c_void,
                            kind,
                            installed.as_ptr().cast(),
                            installed.len(),
                        )
                    };
                }
            }
            Err(failure) => log(ERROR, "marketplace", &format!("Failed to install {identifier}: {failure}")),
        }
        notify.changed();
    });
}

fn text<'a>(data: *const c_char, length: usize) -> &'a str {
    std::str::from_utf8(bytes(data, length)).unwrap_or("")
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_marketplace_configure(
    settings: *const AstralSettingsHandle,
    state: *const c_char,
    state_length: usize,
    resources: *const c_char,
    resources_length: usize,
    key: *const c_char,
    key_length: usize,
    fetch: Option<Fetch>,
    fetch_context: *mut c_void,
    installed: Option<Installed>,
    installed_context: *mut c_void,
) {
    let Some(fetch) = fetch else { return };
    let configured = Config {
        settings: settings as usize,
        state: path(state, state_length).to_path_buf(),
        resources: path(resources, resources_length).to_path_buf(),
        key: bytes(key, key_length).to_vec(),
        fetch,
        fetch_context: fetch_context as usize,
        installed,
        installed_context: installed_context as usize,
    };
    *CONFIG.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Arc::new(configured));
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_marketplace_catalog() -> AstralRustString {
    AstralRustString::new(with_catalog(|catalog| catalog.entries.to_string()).into_bytes())
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_marketplace_state() -> AstralRustString {
    AstralRustString::new(with_catalog(|catalog| catalog.state).as_bytes().to_vec())
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_marketplace_error() -> AstralRustString {
    AstralRustString::new(with_catalog(|catalog| catalog.error.clone()).into_bytes())
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_marketplace_url() -> AstralRustString {
    AstralRustString::new(config().map(|config| config.catalog_url()).unwrap_or_default().into_bytes())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_marketplace_install_status(identifier: *const c_char, length: usize) -> AstralRustString {
    let identifier = text(identifier, length);
    optional(with_catalog(|catalog| {
        catalog.installs.get(identifier).map(|status| status.as_bytes().to_vec())
    }))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_marketplace_refresh(changed: Option<Changed>, context: *mut c_void, release: Option<Release>) {
    refresh(Notify {
        call: changed,
        context: context as usize,
        release,
    });
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_marketplace_install(
    package: *const c_char,
    length: usize,
    kind: c_int,
    changed: Option<Changed>,
    context: *mut c_void,
    release: Option<Release>,
) {
    let notify = Notify {
        call: changed,
        context: context as usize,
        release,
    };
    let Ok(package) = serde_json::from_slice::<Value>(bytes(package, length)) else {
        return;
    };
    install(package, kind, notify);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_marketplace_stop_list_reason(phoenix: *const c_char, length: usize) -> AstralRustString {
    optional(
        config()
            .and_then(|config| config.stop_list_reason(text(phoenix, length)))
            .map(String::into_bytes),
    )
}
