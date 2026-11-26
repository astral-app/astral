use core::ffi::{CStr, c_char};
use std::ffi::CString;

use serde_json::{Map, Value, json};

const PROPERTY_VALUE_MAX: usize = 92;
const SELINUX_ENFORCE: &str = "/sys/fs/selinux/enforce";
const NATIVE_LIBRARIES: &str = "lib/";
const WAYDROID_PROPERTY: &str = "waydroid.tools_version";
const NATIVE_BRIDGE_PROPERTY: &str = "ro.dalvik.vm.native.bridge";
const VERIFIED_BOOT_PROPERTY: &str = "ro.boot.verifiedbootstate";
const ONE_UI_MAJOR: u32 = 10_000;
const ONE_UI_MINOR: u32 = 100;

const ROM_PROPERTIES: [(&str, &str); 13] = [
    ("ro.lineage.version", "LineageOS"),
    ("ro.crdroid.version", "crDroid"),
    ("ro.evolution.version", "Evolution X"),
    ("ro.pixelexperience.version", "PixelExperience"),
    ("ro.grapheneos.version", "GrapheneOS"),
    ("ro.calyxos.version", "CalyxOS"),
    ("ro.aospa.version", "Paranoid Android"),
    ("ro.arrow.version", "ArrowOS"),
    ("ro.derp.version", "DerpFest"),
    ("ro.rising.version", "RisingOS"),
    ("ro.matrixx.version", "Project Matrixx"),
    ("ro.voltage.version", "VoltageOS"),
    ("ro.modversion", ""),
];

const EMULATOR_FILES: [(&str, &str); 11] = [
    ("/data/.bluestacks.prop", "BlueStacks"),
    ("/system/bin/bstfolderd", "BlueStacks"),
    ("/system/lib/egl/libGLES_bst.so", "BlueStacks"),
    ("/system/bin/nox-prop", "Nox"),
    ("/system/bin/ldinit", "LDPlayer"),
    ("/dev/socket/genyd", "Genymotion"),
    ("/system/bin/microvirt-prop", "MEmu"),
    ("/system/lib/libmicrovirt.so", "MEmu"),
    ("/system/bin/nemuVM-prop", "MuMu"),
    ("/system/lib/libnemuVMprop.so", "MuMu"),
    ("/system/bin/androVM-prop", "Genymotion"),
];

type Skin<'a> = (&'a str, &'a dyn Fn(&str) -> String);

unsafe extern "C" {
    fn __system_property_get(name: *const c_char, value: *mut c_char) -> i32;
}

fn property(key: &str) -> Option<String> {
    let name = CString::new(key).ok()?;
    let mut value = [0 as c_char; PROPERTY_VALUE_MAX];
    let length = unsafe { __system_property_get(name.as_ptr(), value.as_mut_ptr()) };
    if length <= 0 {
        return None;
    }
    let text = unsafe { CStr::from_ptr(value.as_ptr()) }
        .to_string_lossy()
        .trim()
        .to_owned();
    (!text.is_empty()).then_some(text)
}

fn one_ui(version: &str) -> String {
    version.parse::<u32>().map_or_else(
        |_| format!("One UI {version}"),
        |code| {
            format!(
                "One UI {}.{}",
                code / ONE_UI_MAJOR,
                code % ONE_UI_MAJOR / ONE_UI_MINOR
            )
        },
    )
}

fn vendor_skin() -> String {
    let skins: [Skin; 8] = [
        ("ro.mi.os.version.name", &|value| {
            format!("HyperOS {}", value.trim_start_matches("OS"))
        }),
        ("ro.miui.ui.version.name", &|value| {
            format!("MIUI {}", value.trim_start_matches('V'))
        }),
        ("ro.build.version.oneui", &one_ui),
        ("ro.build.version.oplusrom", &|value| {
            format!("ColorOS {}", value.trim_start_matches('V'))
        }),
        ("ro.build.version.opporom", &|value| {
            format!("ColorOS {}", value.trim_start_matches('V'))
        }),
        ("hw_sc.build.platform.version", &|value| {
            format!("HarmonyOS {value}")
        }),
        ("ro.build.version.emui", &|value| {
            format!(
                "EMUI {}",
                value.split_once('_').map_or(value, |(_, version)| version)
            )
        }),
        ("ro.vivo.os.version", &|value| {
            format!(
                "{} {value}",
                property("ro.vivo.os.name").unwrap_or_else(|| "Funtouch OS".to_owned())
            )
        }),
    ];
    skins
        .iter()
        .find_map(|(key, label)| property(key).map(|value| label(&value)))
        .unwrap_or_default()
}

fn custom_rom() -> String {
    ROM_PROPERTIES
        .iter()
        .find_map(|(key, name)| {
            property(key).map(|version| {
                [*name, version.as_str()]
                    .iter()
                    .filter(|part| !part.is_empty())
                    .copied()
                    .collect::<Vec<_>>()
                    .join(" ")
            })
        })
        .unwrap_or_default()
}

fn selinux_enforcing() -> Option<bool> {
    std::fs::read_to_string(SELINUX_ENFORCE)
        .ok()
        .map(|value| value.trim() == "1")
}

fn emulator() -> Option<String> {
    if property(WAYDROID_PROPERTY).is_some() {
        return Some("Waydroid".to_owned());
    }
    EMULATOR_FILES
        .iter()
        .find(|(path, _)| std::path::Path::new(path).exists())
        .map(|(_, name)| (*name).to_owned())
}

fn page_size() -> libc::c_long {
    unsafe { libc::sysconf(libc::_SC_PAGESIZE) }
}

fn is_merged_apk(source: &str, has_splits: bool) -> bool {
    if has_splits || source.is_empty() {
        return false;
    }
    let Ok(file) = std::fs::File::open(source) else {
        return false;
    };
    zip::ZipArchive::new(file)
        .map(|archive| {
            archive
                .file_names()
                .any(|name| name.is_ok_and(|name| name.starts_with(NATIVE_LIBRARIES)))
        })
        .unwrap_or(false)
}

fn merge_device(device: &mut Map<String, Value>) {
    let platform_host = device
        .remove("hostFeature")
        .and_then(|host| host.as_str().map(str::to_owned));
    let virtual_host = device
        .remove("virtualHost")
        .and_then(|host| host.as_str().map(str::to_owned));
    let host = platform_host
        .filter(|host| !host.is_empty())
        .or_else(emulator)
        .or(virtual_host.filter(|host| !host.is_empty()))
        .unwrap_or_default();
    let selinux = selinux_enforcing().map_or_else(
        || device.remove("selinuxFallback").unwrap_or(Value::Null),
        Value::Bool,
    );
    device.remove("selinuxFallback");
    for (key, value) in [
        ("host", json!(host)),
        ("selinux", selinux),
        (
            "verifiedBoot",
            json!(property(VERIFIED_BOOT_PROPERTY).unwrap_or_default()),
        ),
        ("rom", json!(custom_rom())),
        ("skin", json!(vendor_skin())),
        (
            "abiBridge",
            json!(property(NATIVE_BRIDGE_PROPERTY).unwrap_or_default()),
        ),
        ("pageSize", json!(page_size())),
    ] {
        device.insert(key.to_owned(), value);
    }
}

fn merge_install(install: &mut Map<String, Value>) {
    let source = install
        .remove("sourceDir")
        .and_then(|source| source.as_str().map(str::to_owned))
        .unwrap_or_default();
    let has_splits = install.get("splits").and_then(Value::as_u64).unwrap_or(0) > 0;
    install.insert(
        "merged".to_owned(),
        json!(is_merged_apk(&source, has_splits)),
    );
}

pub fn merge(platform: &str) -> String {
    let mut facts = serde_json::from_str::<Value>(platform).unwrap_or_else(|_| json!({}));
    if let Some(device) = facts.get_mut("device").and_then(Value::as_object_mut) {
        merge_device(device);
    }
    if let Some(install) = facts.get_mut("install").and_then(Value::as_object_mut) {
        merge_install(install);
    }
    facts.to_string()
}
