use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::env_report::{self, AppleFacts, HttpsCheck, LocaleFacts};
use super::environment::{self, InstallFacts, RECEIPT_NONE, SideloaderMarkers, SigningFacts};

const JAILBREAK_MARKERS: [&str; 8] = [
    ".installed_relaxin",
    ".installed_nekojb",
    ".installed_nathanlr",
    ".palecursus_strapped",
    ".installed_palera1n",
    ".installed_dopamine",
    ".xia0o0o0o_jb_installed",
    "basebins",
];
const DYNAMIC_ISLAND_SUBTYPES: [i64; 4] = [2556, 2622, 2796, 2868];
const FIRST_ISLAND_MAJOR: i64 = 15;
const ISLAND_MACHINE: &str = "iPhone14,6";
const IPHONE: &str = "iPhone";
const ROOTLESS: &str = "/var/jb";
const APP_SYNC: &str = "/Library/MobileSubstrate/DynamicLibraries/AppSyncUnified-installd.dylib";
const TROLL_STORE: &str = "../_TrollStore";
const TROLL_STORE_LITE: &str = "../_TrollStoreLite";
const TROLL_FOOLS_SUFFIX: &str = ".troll-fools.bak";
const LIVE_PROCESS: &str = "LiveProcess";
const SYSTEM_IMAGES: [&str; 2] = ["/System/", "/usr/lib/system/"];
const DPKG_STATUSES: [&str; 3] = ["/var/jb/Library/dpkg/status", "/var/jb/var/lib/dpkg/status", "/Library/dpkg/status"];

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct HookProvider {
    pub path: String,
    pub libhooker: bool,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Facts {
    pub bundle_path: String,
    pub executable_path: String,
    pub documents_path: String,
    pub image_path: String,
    pub images: Vec<String>,
    pub bootstrap_root: String,
    pub roothide: bool,
    pub hook_provider: Option<HookProvider>,
    pub machine: String,
    pub os_build: String,
    pub device_name: String,
    pub artwork_subtype: i64,
    pub simulator: bool,
    pub code_signing_debugged: bool,
    pub debugger_attached: bool,
    pub side_store_vpn: bool,
    pub kernel_entitlements: String,
    pub receipt_name: Option<String>,
    pub receipt_exists: bool,
    pub live_container_classes: bool,
    pub url_schemes: Vec<String>,
    pub exported_types: Vec<String>,
    pub alt_bundle_identifier: bool,
    pub alt_device_identifier: bool,
    pub release_channel: Option<String>,
    pub lockdown_mode: bool,
    #[serde(rename = "iPhoneAppOnIPad")]
    pub iphone_app_on_ipad: bool,
    #[serde(rename = "iOSAppOnMac")]
    pub ios_app_on_mac: bool,
    #[serde(rename = "iOSAppOnVision")]
    pub ios_app_on_vision: bool,
    pub managed_app: bool,
    pub refresh_rate: i64,
    pub bold_text: bool,
    pub locale: LocaleFacts,
    pub duplicate_copy: String,
}

struct Signature {
    entitlements: plist::Dictionary,
}

static STATIC_REPORT: OnceLock<Map<String, Value>> = OnceLock::new();

fn exists(path: impl AsRef<Path>) -> bool {
    path.as_ref().exists()
}

fn resolved(path: &str) -> String {
    std::fs::canonicalize(path).map_or_else(|_| path.to_owned(), |resolved| resolved.to_string_lossy().into_owned())
}

fn trimmed_file(path: impl AsRef<Path>) -> String {
    std::fs::read_to_string(path).map(|text| text.trim().to_owned()).unwrap_or_default()
}

fn bootstrap_root(facts: &Facts) -> Option<String> {
    if !facts.bootstrap_root.is_empty() {
        return Some(resolved(&facts.bootstrap_root));
    }
    std::iter::once(&facts.image_path)
        .chain(&facts.images)
        .find_map(|image| environment::bootstrap_root_of_image(resolved(image).as_bytes()))
        .map(|root| String::from_utf8_lossy(&root).into_owned())
}

fn jailbreak(facts: &Facts) -> String {
    let Some(root) = bootstrap_root(facts) else { return String::new() };
    let markers: Vec<Vec<u8>> = JAILBREAK_MARKERS
        .iter()
        .filter(|marker| exists(Path::new(&root).join(marker)))
        .map(|marker| marker.as_bytes().to_vec())
        .collect();
    let images: Vec<Vec<u8>> = facts.images.iter().map(|image| resolved(image).into_bytes()).collect();
    let name = environment::jailbreak_name(root.as_bytes(), &markers, &images, facts.roothide);
    let version = trimmed_file(Path::new(&root).join("basebin/.version"));
    String::from_utf8_lossy(&environment::jailbreak_label(name.as_bytes(), version.as_bytes())).into_owned()
}

fn model_edit(facts: &Facts) -> String {
    let Some(model) = facts.machine.strip_prefix(IPHONE) else {
        return String::new();
    };
    if !DYNAMIC_ISLAND_SUBTYPES.contains(&facts.artwork_subtype) {
        return String::new();
    }
    let major: i64 = model.split(',').next().and_then(|major| major.parse().ok()).unwrap_or(0);
    if major >= FIRST_ISLAND_MAJOR && facts.machine != ISLAND_MACHINE {
        return String::new();
    }
    "Dynamic Island added by a MobileGestalt edit".to_owned()
}

fn rootless_dopamine_version() -> String {
    let version = trimmed_file(Path::new(ROOTLESS).join("basebin/.version"));
    if exists(Path::new(ROOTLESS).join(".installed_dopamine")) && !version.is_empty() {
        format!("Dopamine {version}")
    } else {
        String::new()
    }
}

fn bundle_contains_suffix(bundle: &str, suffix: &str) -> bool {
    std::fs::read_dir(bundle)
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| entry.file_name().to_string_lossy().ends_with(suffix))
}

fn has_app_sync() -> bool {
    ["", ROOTLESS].iter().any(|root| exists(format!("{root}{APP_SYNC}")))
}

fn apple_facts(facts: &Facts, jailbroken: bool) -> AppleFacts {
    AppleFacts {
        model_edit: model_edit(facts),
        jailbreak_version: rootless_dopamine_version(),
        ios_app_on_vision: facts.ios_app_on_vision,
        side_store_vpn: facts.side_store_vpn,
        live_container_multitask: facts.executable_path.contains(LIVE_PROCESS),
        refresh_rate: facts.refresh_rate,
        bold_text: facts.bold_text,
        locale: LocaleFacts {
            calendar: facts.locale.calendar.clone(),
            non_latin_digits: facts.locale.non_latin_digits,
            right_to_left: facts.locale.right_to_left,
        },
        lockdown_mode: facts.lockdown_mode,
        iphone_app_on_ipad: facts.iphone_app_on_ipad,
        ios_app_on_mac: facts.ios_app_on_mac,
        play_cover: facts.images.iter().any(|image| image.contains("/PlayTools.framework/")),
        managed_app: facts.managed_app,
        jit_enabled: !jailbroken && facts.code_signing_debugged,
        debugger_attached: facts.debugger_attached,
        troll_fools: bundle_contains_suffix(&facts.bundle_path, TROLL_FOOLS_SUFFIX),
        app_sync: has_app_sync(),
        duplicate_copy: facts.duplicate_copy.clone(),
    }
}

fn entitlements_plist(plist: &[u8]) -> Option<plist::Dictionary> {
    plist::Value::from_reader_xml(plist)
        .ok()?
        .into_dictionary()
        .filter(|entitlements| !entitlements.is_empty())
}

fn signature(facts: &Facts) -> Option<Signature> {
    let kernel = crate::lua::host::install::decode_base64(&facts.kernel_entitlements).unwrap_or_default();
    if let Some(entitlements) = super::macho::entitlements_from_blob(&kernel).and_then(|plist| entitlements_plist(&plist)) {
        return Some(Signature { entitlements });
    }
    super::macho::entitlement_blobs(Path::new(&facts.executable_path))
        .iter()
        .find_map(|blob| entitlements_plist(blob))
        .map(|entitlements| Signature { entitlements })
}

fn team_identifier(signature: Option<&Signature>) -> Option<String> {
    signature?
        .entitlements
        .get("com.apple.developer.team-identifier")?
        .as_string()
        .map(str::to_owned)
}

struct Profile {
    provisions_all_devices: bool,
    lists_devices: bool,
    team_name: Option<String>,
    expiry: Option<String>,
}

fn profile(bundle: &str) -> Option<Profile> {
    let data = std::fs::read(Path::new(bundle).join("embedded.mobileprovision")).ok()?;
    let plist = environment::provisioning_profile_plist(&data)?;
    let profile = plist::Value::from_reader_xml(plist.as_slice()).ok()?.into_dictionary()?;
    Some(Profile {
        provisions_all_devices: profile
            .get("ProvisionsAllDevices")
            .and_then(plist::Value::as_boolean)
            .unwrap_or(false),
        lists_devices: profile
            .get("ProvisionedDevices")
            .and_then(plist::Value::as_array)
            .is_some_and(|devices| !devices.is_empty()),
        team_name: profile.get("TeamName").and_then(plist::Value::as_string).map(str::to_owned),
        expiry: profile
            .get("ExpirationDate")
            .and_then(plist::Value::as_date)
            .map(|date| date.to_xml_format().chars().take("YYYY-MM-DD".len()).collect()),
    })
}

fn strings(values: &[String]) -> Vec<Vec<u8>> {
    values.iter().map(|value| value.clone().into_bytes()).collect()
}

fn install_details(facts: &Facts, signature: Option<&Signature>) -> String {
    let team = team_identifier(signature);
    let get_task_allow = signature
        .and_then(|signature| signature.entitlements.get("get-task-allow"))
        .and_then(plist::Value::as_boolean)
        .unwrap_or(false);
    let profile = profile(&facts.bundle_path);
    let (schemes, types) = (strings(&facts.url_schemes), strings(&facts.exported_types));
    let sideloader = environment::sideloader_name(&SideloaderMarkers {
        url_schemes: &schemes,
        exported_types: &types,
        alt_bundle_identifier: facts.alt_bundle_identifier,
        alt_device_identifier: facts.alt_device_identifier,
    });
    let details = environment::signing_details(&SigningFacts {
        team_identifier: team.as_deref().map(str::as_bytes),
        get_task_allow,
        has_profile: profile.is_some(),
        provisions_all_devices: profile.as_ref().is_some_and(|profile| profile.provisions_all_devices),
        lists_devices: profile.as_ref().is_some_and(|profile| profile.lists_devices),
        profile_expiry: profile.as_ref().and_then(|profile| profile.expiry.as_deref()).map(str::as_bytes),
        team_name: profile.as_ref().and_then(|profile| profile.team_name.as_deref()).map(str::as_bytes),
        sideloader: sideloader.as_bytes(),
    });
    String::from_utf8_lossy(&details).into_owned()
}

fn live_container(facts: &Facts) -> bool {
    if facts.live_container_classes {
        return true;
    }
    let (executable, bundle) = (resolved(&facts.executable_path), resolved(&facts.bundle_path));
    !bundle.is_empty() && !executable.is_empty() && !executable.starts_with(&format!("{bundle}/"))
}

fn app_source(facts: &Facts, signature: Option<&Signature>, jailbroken: bool) -> &'static str {
    let bundle = Path::new(&facts.bundle_path);
    let team = team_identifier(signature);
    environment::app_source(&InstallFacts {
        simulator: facts.simulator,
        receipt: facts.receipt_name.as_deref().map_or(RECEIPT_NONE, |name| {
            environment::receipt_kind(Some(name.as_bytes()), facts.receipt_exists)
        }),
        troll_store: exists(bundle.join(TROLL_STORE)),
        troll_store_lite: exists(bundle.join(TROLL_STORE_LITE)),
        live_container: live_container(facts),
        jit: jailbroken || facts.code_signing_debugged,
        discord_signed: team.map(|team| environment::is_discord_production_team(Some(team.as_bytes()))),
    })
}

fn hook_framework_version(package: &str, image: &str, root: Option<&str>) -> String {
    let mut statuses: Vec<PathBuf> = DPKG_STATUSES.iter().map(PathBuf::from).collect();
    if let Some(root) = root {
        statuses.insert(0, Path::new(root).join("var/lib/dpkg/status"));
    }
    for status in statuses {
        if let Ok(text) = std::fs::read(&status)
            && let Some(version) = environment::dpkg_package_version(&text, package.as_bytes())
        {
            return String::from_utf8_lossy(&version).into_owned();
        }
    }
    let info = Path::new(image).parent().map(|folder| folder.join("Info.plist"));
    info.and_then(|info| plist::Value::from_file(info).ok())
        .and_then(plist::Value::into_dictionary)
        .and_then(|info| {
            info.get("CFBundleShortVersionString")
                .and_then(plist::Value::as_string)
                .map(str::to_owned)
        })
        .unwrap_or_default()
}

fn framework_label(image: &str, libhooker: bool, root: Option<&str>) -> String {
    let path = resolved(image);
    let Some((name, package)) = environment::hook_framework_for_image(path.as_bytes(), libhooker) else {
        return String::new();
    };
    let version = hook_framework_version(package, &path, root);
    String::from_utf8_lossy(&environment::framework_label(name.as_bytes(), version.as_bytes())).into_owned()
}

fn injection_framework(facts: &Facts) -> String {
    let root = bootstrap_root(facts);
    if let Some(provider) = &facts.hook_provider {
        let label = framework_label(&provider.path, provider.libhooker, root.as_deref());
        if !label.is_empty() {
            return label;
        }
    }
    facts
        .images
        .iter()
        .filter(|image| !SYSTEM_IMAGES.iter().any(|system| image.contains(system)))
        .map(|image| framework_label(image, false, root.as_deref()))
        .find(|label| !label.is_empty())
        .unwrap_or_default()
}

fn static_report(facts: &Facts) -> Map<String, Value> {
    let mut report = Map::new();
    let signature = signature(facts);
    let jailbreak = jailbreak(facts);
    if let Some(anatomy) = super::macho::read_anatomy(Path::new(&facts.executable_path)) {
        report.insert(
            "binary".into(),
            json!(super::macho::anatomy_line(&anatomy, facts.bundle_path.as_bytes())),
        );
        if let Some(signer) = super::macho::signer(&anatomy.signature) {
            report.insert("signer".into(), json!(super::macho::signer_line(&signer)));
        }
    }
    if !facts.os_build.is_empty() {
        report.insert("os_build".into(), json!(facts.os_build));
    }
    if env_report::is_beta_build(&facts.os_build) {
        report.insert("os_beta".into(), json!(facts.os_build));
    }
    let device = if facts.device_name.is_empty() {
        &facts.machine
    } else {
        &facts.device_name
    };
    report.insert("device_name".into(), json!(device));
    report.insert("source".into(), json!(app_source(facts, signature.as_ref(), !jailbreak.is_empty())));
    let install = install_details(facts, signature.as_ref());
    if !install.is_empty() {
        report.insert("install".into(), json!(install));
    }
    let image = Path::new(&facts.image_path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned());
    report.insert("image".into(), json!(image.unwrap_or_default()));
    report.insert("framework".into(), json!(injection_framework(facts)));
    report.insert("jailbreak".into(), json!(jailbreak));
    if let Some(channel) = &facts.release_channel {
        report.insert("discord_channel".into(), json!(channel));
    }
    report
}

const PASSED_THROUGH: [&str; 2] = ["compatibility", "phoenix"];

pub fn report(facts: &Value) -> Value {
    let facts_value = facts;
    let facts: Facts = Facts::deserialize(facts).unwrap_or_else(|error| {
        crate::app::host::log(&format!("Failed to read the iOS environment facts: {error}"));
        Facts::default()
    });
    let mut environment = STATIC_REPORT.get_or_init(|| static_report(&facts)).clone();
    let jailbroken = environment
        .get("jailbreak")
        .and_then(Value::as_str)
        .is_some_and(|name| !name.is_empty());
    let mut markers = env_report::apple_markers(&apple_facts(&facts, jailbroken));
    let stack = super::super::network::stack::marker(&super::super::network::stack::interface_addresses());
    if !stack.is_empty() {
        markers.push(stack.to_owned());
    }
    let leftover = super::leftover_mods::marker(Path::new(&facts.documents_path));
    if !leftover.is_empty() {
        markers.push(leftover);
    }
    environment.insert("markers".into(), json!(markers));
    for key in PASSED_THROUGH {
        if let Some(value) = facts_value.get(key) {
            environment.insert(key.into(), value.clone());
        }
    }
    let https: HttpsCheck = HttpsCheck::deserialize(env_report::current_https_check()).unwrap_or_default();
    if https.completed || !https.failure.is_empty() {
        environment.insert("https".into(), env_report::https_verdict(&https));
    }
    Value::Object(environment)
}
