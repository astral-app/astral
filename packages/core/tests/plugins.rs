mod support;

use std::path::Path;

use astral_core::plugins::packs::{
    AstralPacksHandle, astral_packs_emoji_url, astral_packs_icon_path, astral_packs_list, astral_packs_open, astral_packs_select,
};
use astral_core::plugins::signatures::{signed_files, verify, verify_directory};
use astral_core::plugins::themes::{astral_themes_import_unbound, astral_themes_list, astral_themes_select};
use astral_core::plugins::trace::{
    AstralPreviousSession, astral_trace_close, astral_trace_enter, astral_trace_leave, astral_trace_open, astral_trace_set_running,
};
use astral_core::settings::store::AstralSettingsHandle;
use astral_core::support::bridge::{AstralRustString, take};
use astral_core::support::crypto::{hex, sha256};
use serde_json::{Value, json};
use support::{configure_pages, on_page, reset_pages, setting_flag, settings, write};

const PACK_ICONS: core::ffi::c_int = 0;
const PACK_EMOJI: core::ffi::c_int = 1;
const ROW_INFO: i64 = 2;
const SURFACE_TEXT: usize = 1;
const SIX_HOURS: i64 = 6 * 60 * 60;

fn text(string: AstralRustString) -> String {
    String::from_utf8_lossy(&take(string)).into_owned()
}

fn json_of(string: AstralRustString) -> Value {
    serde_json::from_slice(&take(string)).unwrap_or(Value::Null)
}

struct Packs(*mut AstralPacksHandle);

impl Packs {
    fn open(settings: *const AstralSettingsHandle, local: &Path, bundled: &Path) -> Packs {
        let (local, bundled) = (local.to_string_lossy().into_owned(), bundled.to_string_lossy().into_owned());
        Packs(unsafe { astral_packs_open(settings, local.as_ptr().cast(), local.len(), bundled.as_ptr().cast(), bundled.len()) })
    }

    fn list(&self, kind: core::ffi::c_int) -> Value {
        json_of(unsafe { astral_packs_list(self.0, kind) })
    }

    fn select(&self, kind: core::ffi::c_int, identifier: &str) -> bool {
        unsafe { astral_packs_select(self.0, kind, identifier.as_ptr().cast(), identifier.len()) != 0 }
    }

    fn emoji(&self, codepoints: &str) -> String {
        text(unsafe { astral_packs_emoji_url(self.0, codepoints.as_ptr().cast(), codepoints.len()) })
    }

    fn icon(&self, name: &str) -> String {
        text(unsafe { astral_packs_icon_path(self.0, name.as_ptr().cast(), name.len()) })
    }
}

struct Themes {
    settings: *const AstralSettingsHandle,
    local: String,
    bundled: String,
}

impl Themes {
    fn select(&self, identifier: &str) -> Value {
        json_of(unsafe {
            astral_themes_select(
                self.settings,
                self.local.as_ptr().cast(),
                self.local.len(),
                self.bundled.as_ptr().cast(),
                self.bundled.len(),
                identifier.as_ptr().cast(),
                identifier.len(),
            )
        })
    }

    fn list(&self) -> Value {
        json_of(unsafe {
            astral_themes_list(
                self.local.as_ptr().cast(),
                self.local.len(),
                self.bundled.as_ptr().cast(),
                self.bundled.len(),
            )
        })
    }

    fn import(&self, source: &str) -> Option<String> {
        let fallback = "fallback";
        let error = take(unsafe {
            astral_themes_import_unbound(
                self.settings,
                self.local.as_ptr().cast(),
                self.local.len(),
                self.bundled.as_ptr().cast(),
                self.bundled.len(),
                source.as_ptr().cast(),
                source.len(),
                fallback.as_ptr().cast(),
                fallback.len(),
            )
        });
        (!error.is_empty()).then(|| String::from_utf8_lossy(&error).into_owned())
    }

    fn active(&self) -> String {
        unsafe { astral_core::settings::store::store(self.settings) }
            .and_then(|mut store| store.get_string("astral", "theme"))
            .unwrap_or_else(|| "discord".to_owned())
    }
}

fn colors(style: &Value, key: &str) -> Vec<u64> {
    style[key].as_array().into_iter().flatten().filter_map(Value::as_u64).collect()
}

fn shared_packs(work: &Path) {
    let (bundled, local) = (work.join("packs/bundled"), work.join("packs/local"));
    write(
        &bundled.join("emoji-packs/sample/manifest.toml"),
        "id = 'sample'\nname = 'Bundled'\nstatic = 'https://example.com/bundled/{codepoints}.png'\n",
    );
    write(
        &local.join("emoji-packs/sample/manifest.toml"),
        "id = 'sample'\nname = 'Local'\nstatic = 'https://example.com/static/{codepoints}.png'\nanimated = 'https://example.com/animated/{codepoints}.gif'\n",
    );
    write(&local.join("emoji-packs/sample/coverage.txt"), "1f600\r\n1f469+1f4bb\n2764!\n");
    write(&local.join("emoji-packs/sample/animated.txt"), "1f600\n");
    write(&bundled.join("icon-packs/sample/manifest.toml"), "id = 'sample'\nname = 'Icons'\n");
    write(&bundled.join("icon-packs/sample/icons/ic_test@2x.png"), "two");
    let handle = settings(&local);
    let packs = Packs::open(handle, &local, &bundled);
    let emoji = packs.list(PACK_EMOJI);
    assert_eq!(emoji.as_array().map(Vec::len), Some(2));
    assert_eq!(emoji[1]["name"], "Local");
    assert!(packs.emoji("1f600").is_empty());
    assert!(!packs.select(PACK_EMOJI, "missing"));
    assert!(packs.select(PACK_EMOJI, "sample") && packs.select(PACK_EMOJI, "sample"));
    assert_eq!(packs.emoji("1F600"), "https://example.com/animated/1f600.gif");
    support::set_setting(handle, "astral", "emoji-animated", json!(false));
    assert_eq!(packs.emoji("1f600"), "https://example.com/static/1f600.png");
    assert_eq!(packs.emoji("1f469-200d-1f4bb"), "https://example.com/static/1f469-200d-1f4bb.png");
    assert_eq!(packs.emoji("2764-fe0f"), "https://example.com/static/2764-fe0f.png");
    assert!(packs.emoji("1f601").is_empty() && packs.emoji("../../secret").is_empty());
    assert!(packs.select(PACK_ICONS, "sample"));
    assert_eq!(
        packs.icon("ic_test"),
        bundled.join("icon-packs/sample/icons/ic_test@2x.png").to_string_lossy()
    );
    assert!(packs.icon("ic_../../secret").is_empty());
    assert!(packs.select(PACK_ICONS, "discord") && packs.icon("ic_test").is_empty());
    assert!(packs.select(PACK_EMOJI, "discord") && packs.emoji("1f600").is_empty());
    assert!(astral_core::support::toml_json::parse("invalid = [").is_none());
}

fn theme_parity(work: &Path) {
    let (bundled, local) = (work.join("themes/bundled"), work.join("themes/local"));
    std::fs::create_dir_all(&local).expect("local");
    write(
        &bundled.join("themes/sample.toml"),
        "id = 'sample'\nname = 'Sample'\n[colors]\nFF202020 = 'FF302040'\n[tint]\ncolor = '#8B5CF6'\n[tint.text]\ncolor = '#EEEEFF'\nsaturation = 0.2\n[background]\ngradient = ['#112233', '#445566']\n",
    );
    write(
        &bundled.join("palettes/discord-raw.toml"),
        "[colors]\nDARK = '#202020'\nLIGHT = '#EEEEEE'\n",
    );
    write(
        &bundled.join("palettes/discord-semantic.toml"),
        "[colors.background]\ndark = ['DARK', 1]\nlight = ['LIGHT', 1]\n",
    );
    let handle = settings(&local);
    let themes = Themes {
        settings: handle,
        local: local.to_string_lossy().into_owned(),
        bundled: bundled.to_string_lossy().into_owned(),
    };
    let style = themes.select("sample");
    assert_eq!(colors(&style, "from"), vec![0xFF20_2020, 0xFFEE_EEEE]);
    assert_eq!(colors(&style, "to"), vec![0xFF30_2040, 0xFF30_2040]);
    assert_eq!(style["tint"].as_u64(), Some(0xFF8B_5CF6));
    assert_eq!(style["surfaces"][SURFACE_TEXT]["color"].as_u64(), Some(0xFFEE_EEFF));
    assert_eq!(colors(&style, "gradient"), vec![0xFF11_2233, 0xFF44_5566]);
    assert_eq!(themes.active(), "sample");
    assert!(colors(&themes.select("missing"), "from").is_empty() && themes.active() == "discord");
    assert!(
        themes
            .import(r##"{"name":"My theme","semanticColors":{"background":["#ABCDEF"]}}"##)
            .is_none()
    );
    assert_eq!(themes.active(), "unbound-my-theme");
    assert!(
        themes
            .import(r##"{"name":"My theme","semanticColors":{"background":["#AABBCC"]}}"##)
            .is_none()
    );
    assert_eq!(colors(&themes.select(&themes.active()), "to")[0], 0xFFAA_BBCC);
    assert!(themes.import("[]").is_some());
    let listed = themes.list();
    let active = themes.active();
    on_page("themes", move |_, _| listed.clone());
    on_page("activeTheme", move |_, _| json!(active));
    configure_pages(handle, "0.0.1");
    let design = astral_core::settings::pages::model("design");
    for section in design["sections"].as_array().into_iter().flatten() {
        assert!(section["rows"].as_array().is_some_and(|rows| rows.iter().all(Value::is_object)));
    }
    let mut converted = false;
    astral_core::settings::navigator::with_spec(&design, |spec| {
        let title = unsafe { core::ffi::CStr::from_ptr(spec.title) };
        assert_eq!(title.to_str(), Ok("Design"));
        assert_eq!(spec.count, 5);
        converted = true;
    });
    assert!(converted);
    reset_pages();
}

fn singleton_page_collections() {
    let row = json!({"key": "theme-note", "title": "Theme note", "kind": ROW_INFO});
    let mut model = json!({"title": "Design", "sections": {"title": "How themes apply", "rows": row}});
    let mut converted = false;
    astral_core::settings::navigator::with_spec(&model, |spec| {
        let section = unsafe { &*spec.sections };
        let first = unsafe { &*section.rows };
        assert_eq!((spec.count, section.count), (1, 1));
        assert_eq!(unsafe { core::ffi::CStr::from_ptr(first.title) }.to_str(), Ok("Theme note"));
        converted = true;
    });
    assert!(converted);
    model["sections"] = json!(["invalid", {"rows": [row, "invalid"]}]);
    astral_core::settings::navigator::with_spec(&model, |spec| {
        assert_eq!((spec.count, unsafe { &*spec.sections }.count), (1, 1));
    });
}

fn catalog_updates(work: &Path) {
    use astral_core::settings::pages::{action, catalog_refresh_due, refresh_catalog_if_due};
    assert!(catalog_refresh_due(1000, 0));
    assert!(!catalog_refresh_due(1000 + 60, 1000));
    assert!(catalog_refresh_due(1000 + SIX_HOURS, 1000));
    assert!(catalog_refresh_due(500, 1000));
    let root = work.join("catalog-updates");
    std::fs::create_dir_all(&root).expect("root");
    let handle = settings(&root);
    let (installs, notices, refreshes) = (
        support::recorder::<String>(),
        support::recorder::<String>(),
        support::recorder::<()>(),
    );
    on_page(
        "catalog",
        |_, _| json!({"astral": {"version": "0.0.2"}, "themes": [{"id": "astral.theme", "name": "Theme", "version": "2.0"}]}),
    );
    on_page("themes", |_, _| json!([{"id": "astral.theme", "version": "1.0"}]));
    let installed = installs.clone();
    on_page("installPackage", move |arguments, _| {
        let package = arguments[0]["id"].as_str().unwrap_or_default();
        installed
            .lock()
            .expect("installs")
            .push(format!("{}:{package}", arguments[1].as_str().unwrap_or_default()));
        Value::Null
    });
    let refreshed = refreshes.clone();
    on_page("refreshCatalog", move |_, reply| {
        refreshed.lock().expect("refreshes").push(());
        if let Some(reply) = reply {
            reply(None);
        }
        Value::Null
    });
    let noticed = notices.clone();
    on_page("notify", move |arguments, _| {
        noticed
            .lock()
            .expect("notices")
            .push(arguments[0].as_str().unwrap_or_default().to_owned());
        Value::Null
    });
    configure_pages(handle, "0.0.1");
    let changes = support::recorder::<()>();
    let counted = changes.clone();
    let changed: astral_core::support::host_callback::Changed = std::sync::Arc::new(move || counted.lock().expect("changes").push(()));
    refresh_catalog_if_due(10_000, true, changed.clone());
    assert_eq!((support::recorded(&refreshes).len(), support::recorded(&changes).len()), (1, 1));
    assert_eq!(support::recorded(&installs), vec!["theme:astral.theme".to_owned()]);
    assert_eq!(support::recorded(&notices), vec!["astral 0.0.2 is available".to_owned()]);
    refresh_catalog_if_due(10_060, false, changed.clone());
    assert_eq!(support::recorded(&refreshes).len(), 1);
    refresh_catalog_if_due(10_000 + SIX_HOURS, false, changed.clone());
    assert_eq!((support::recorded(&refreshes).len(), support::recorded(&notices).len()), (2, 1));
    action("updates", "switch.auto-update", support::changed());
    assert!(!setting_flag(handle, "astral", "auto-update", true));
    let before = support::recorded(&installs).len();
    refresh_catalog_if_due(0, true, changed);
    assert_eq!(support::recorded(&installs).len(), before);
    reset_pages();
}

#[test]
fn packs_themes_and_pages() {
    let work = support::work("plugins");
    shared_packs(&work);
    theme_parity(&work);
    singleton_page_collections();
    catalog_updates(&work);
}

fn digest(contents: &[u8]) -> Vec<u8> {
    hex(&sha256(contents)).into_bytes()
}

#[test]
fn signatures_cover_catalog_and_plugins() {
    let repository = support::repository();
    let key = std::fs::read_to_string(repository.join("keys/plugin-signing.pub")).expect("key");
    let key = key.trim().as_bytes();
    let catalog = std::fs::read(repository.join("packages/catalog/index.toml")).expect("catalog");
    let signature = std::fs::read_to_string(repository.join("packages/catalog/index.toml.sig")).expect("signature");
    assert!(verify(key, &catalog, signature.trim().as_bytes()));
    assert!(!verify(
        key,
        &[catalog.as_slice(), b"tampered"].concat(),
        signature.trim().as_bytes()
    ));
    assert!(!verify(key, &catalog, b"invalid"));
    let plugin = repository.join("packages/addons/plugins/show-me-your-name");
    let directory = support::work("signed-plugin");
    for entry in walk(&plugin) {
        let target = directory.join(entry.strip_prefix(&plugin).expect("relative"));
        std::fs::create_dir_all(target.parent().expect("parent")).expect("folders");
        std::fs::copy(&entry, &target).expect("copy");
    }
    let folder = directory.to_string_lossy().into_owned();
    assert!(verify_directory(key, folder.as_bytes(), "main.lua", digest));
    write(&directory.join("unsigned.lua"), "return true");
    assert!(!verify_directory(key, folder.as_bytes(), "main.lua", digest));
    std::fs::remove_file(directory.join("unsigned.lua")).expect("remove");
    let main = std::fs::read(directory.join("main.lua")).expect("main");
    write(&directory.join("main.lua"), "return true");
    assert!(!verify_directory(key, folder.as_bytes(), "main.lua", digest));
    std::fs::write(directory.join("main.lua"), main).expect("restore");
    let forged = std::fs::read_to_string(plugin.join("signature")).expect("signature");
    assert!(signed_files(key, format!("{forged}\n[files.bad]\nvalue = 'x'\n").as_bytes()).is_none());
    write(&directory.join("signature"), &"x".repeat((128 << 10) + 1));
    assert!(!verify_directory(key, folder.as_bytes(), "main.lua", digest));
    std::fs::remove_file(directory.join("signature")).expect("remove");
    let fifo = std::ffi::CString::new(directory.join("signature").to_string_lossy().as_bytes()).expect("path");
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    assert!(!verify_directory(key, folder.as_bytes(), "main.lua", digest));
}

fn walk(directory: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(directory).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            files.extend(walk(&path));
        } else {
            files.push(path);
        }
    }
    files
}

fn names(string: AstralRustString) -> Vec<String> {
    take(string)
        .split(|byte| *byte == 0)
        .filter(|name| !name.is_empty())
        .map(|name| String::from_utf8_lossy(name).into_owned())
        .collect()
}

fn open_trace(root: &str, now: i64) -> Option<(i64, String, String, Vec<String>)> {
    let mut session = AstralPreviousSession {
        started: 0,
        active: AstralRustString::new(Vec::new()),
        last: AstralRustString::new(Vec::new()),
        running: AstralRustString::new(Vec::new()),
    };
    let found = unsafe { astral_trace_open(root.as_ptr().cast(), root.len(), now, &raw mut session) } != 0;
    let opened = (session.started, text(session.active), text(session.last), names(session.running));
    found.then_some(opened)
}

#[test]
fn plugin_trace_names_the_plugin_that_was_running() {
    let root = support::work("plugin-trace").to_string_lossy().into_owned();
    assert!(open_trace(&root, 100).is_none());
    let running = b"astral.one\0astral.two\0";
    unsafe { astral_trace_set_running(running.as_ptr().cast(), running.len()) };
    unsafe { astral_trace_enter(b"astral.one".as_ptr().cast(), "astral.one".len()) };
    unsafe { astral_trace_enter(b"astral.two".as_ptr().cast(), "astral.two".len()) };
    astral_trace_leave();
    astral_trace_close();
    let crashed = open_trace(&root, 200).expect("crashed");
    assert_eq!(
        crashed,
        (
            100,
            "astral.one".to_owned(),
            "astral.two".to_owned(),
            vec!["astral.one".to_owned(), "astral.two".to_owned()]
        )
    );
    unsafe { astral_trace_enter(b"astral.one".as_ptr().cast(), "astral.one".len()) };
    astral_trace_leave();
    astral_trace_close();
    let clean = open_trace(&root, 300).expect("clean");
    assert_eq!(clean, (200, String::new(), "astral.one".to_owned(), Vec::new()));
    astral_trace_close();
}
