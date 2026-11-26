use core::ffi::{c_char, c_int, c_void};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Sender, channel};
use std::time::Duration;

use astral_core::plugins::marketplace::{
    astral_marketplace_catalog, astral_marketplace_configure, astral_marketplace_install, astral_marketplace_install_status,
    astral_marketplace_refresh, astral_marketplace_state, astral_marketplace_stop_list_reason,
};
use astral_core::settings::store::{astral_settings_open, astral_settings_set};
use astral_core::support::bridge::AstralRustString;

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn take(string: AstralRustString) -> Option<String> {
    if string.bytes.is_null() {
        return None;
    }
    let text = String::from_utf8_lossy(unsafe { core::slice::from_raw_parts(string.bytes, string.length) }).into_owned();
    unsafe { astral_core::support::bridge::astral_rust_string_free(string) };
    Some(text)
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

unsafe extern "C" fn no_network(_: *mut c_void, _: *const c_char, _: usize, _: *mut AstralRustString) -> c_int {
    0
}

unsafe extern "C" fn installed(context: *mut c_void, kind: c_int, identifier: *const c_char, length: usize) {
    let sender = unsafe { &*(context as *const Sender<(c_int, String)>) };
    let identifier = String::from_utf8_lossy(unsafe { core::slice::from_raw_parts(identifier.cast::<u8>(), length) }).into_owned();
    let _ = sender.send((kind, identifier));
}

unsafe extern "C" fn changed(_: *mut c_void) {}

unsafe extern "C" fn print_log(_: *mut c_void, _: c_int, _: *const c_char, _: usize, message: *const c_char, length: usize) {
    eprintln!(
        "log: {}",
        String::from_utf8_lossy(unsafe { core::slice::from_raw_parts(message.cast::<u8>(), length) })
    );
}

fn wait_for(condition: impl Fn() -> bool) {
    for _ in 0..200 {
        if condition() {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("timed out");
}

#[test]
fn installs_signed_plugins_and_themes_from_a_file_catalog() {
    unsafe { astral_core::support::log::astral_set_log_sink(Some(print_log), core::ptr::null_mut(), None) };
    let root = std::env::temp_dir().join(format!("astral-marketplace-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let catalog = root.join("catalog");
    let state = root.join("state");
    std::fs::create_dir_all(&state).unwrap();
    copy_tree(&repository().join("packages/catalog"), &catalog);
    copy_tree(
        &repository().join("packages/addons/plugins/fix-gif-download"),
        &catalog.join("plugins/astral.fix-gif-download"),
    );
    let settings_path = state.join("settings.json").to_string_lossy().into_owned();
    let settings = unsafe { astral_settings_open(settings_path.as_ptr().cast(), settings_path.len(), 0) };
    let url = serde_json::Value::String(format!("file://{}", catalog.join("index.toml").display())).to_string();
    unsafe {
        astral_settings_set(
            settings,
            "astral".as_ptr().cast(),
            6,
            "catalog".as_ptr().cast(),
            7,
            url.as_ptr().cast(),
            url.len(),
        )
    };
    let key = std::fs::read(repository().join("keys/plugin-signing.pub")).unwrap();
    let (sender, receiver) = channel::<(c_int, String)>();
    let sender = Box::leak(Box::new(sender));
    let state_text = state.to_string_lossy().into_owned();
    let resources = repository().join("packages").to_string_lossy().into_owned();
    unsafe {
        astral_marketplace_configure(
            settings,
            state_text.as_ptr().cast(),
            state_text.len(),
            resources.as_ptr().cast(),
            resources.len(),
            key.as_ptr().cast(),
            key.len(),
            Some(no_network),
            core::ptr::null_mut(),
            Some(installed),
            (sender as *mut Sender<(c_int, String)>).cast(),
        );
        astral_marketplace_refresh(Some(changed), core::ptr::null_mut(), None);
    }
    wait_for(|| take(astral_marketplace_state()).as_deref() == Some("ready"));
    let listed: serde_json::Value = serde_json::from_str(&take(astral_marketplace_catalog()).unwrap()).unwrap();
    let plugin = listed["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"] == "astral.fix-gif-download")
        .unwrap()
        .to_string();
    let theme = listed["themes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"] == "mana")
        .unwrap()
        .to_string();
    unsafe {
        astral_marketplace_install(theme.as_ptr().cast(), theme.len(), 1, Some(changed), core::ptr::null_mut(), None);
        astral_marketplace_install(plugin.as_ptr().cast(), plugin.len(), 0, Some(changed), core::ptr::null_mut(), None);
    }
    let mut done = vec![
        receiver.recv_timeout(Duration::from_secs(10)).unwrap(),
        receiver.recv_timeout(Duration::from_secs(10)).unwrap(),
    ];
    done.sort();
    assert_eq!(done, vec![(0, "astral.fix-gif-download".to_owned()), (1, "mana".to_owned())]);
    assert!(state.join("themes/mana.toml").is_file());
    assert!(state.join("plugins/astral.fix-gif-download/main.lua").is_file());
    assert!(!state.join("plugins/.astral.fix-gif-download.installing").exists());
    let status = unsafe { astral_marketplace_install_status("mana".as_ptr().cast(), 4) };
    assert!(take(status).is_none());
    std::fs::write(catalog.join("plugins/astral.fix-gif-download/main.lua"), "return 1").unwrap();
    unsafe { astral_marketplace_install(plugin.as_ptr().cast(), plugin.len(), 0, Some(changed), core::ptr::null_mut(), None) };
    wait_for(|| {
        take(unsafe { astral_marketplace_install_status("astral.fix-gif-download".as_ptr().cast(), 23) }).as_deref() == Some("Failed")
    });
    assert!(state.join("plugins/astral.fix-gif-download/main.lua").is_file());
    assert!(take(unsafe { astral_marketplace_stop_list_reason("not-listed".as_ptr().cast(), 10) }).is_none());
    std::fs::remove_dir_all(root).unwrap();
}
