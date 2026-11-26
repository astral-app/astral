use core::ffi::{c_char, c_int};

use crate::bridge::{AstralRustString, bytes, optional};

pub const RECEIPT_NONE: c_int = 0;
pub const RECEIPT_APP_STORE: c_int = 1;
pub const RECEIPT_TEST_FLIGHT: c_int = 2;

const SANDBOX_RECEIPT_NAME: &[u8] = b"sandboxReceipt";
const DISCORD_TEAM_IDENTIFIER: &[u8] = b"53Q6R32WPB";
const VIRTUAL_DEVICE_MACHINE: &[u8] = b"iPhone99,11";

pub fn receipt_kind(receipt_name: Option<&[u8]>, receipt_exists: bool) -> c_int {
    match receipt_name {
        Some(name) if receipt_exists => {
            if name == SANDBOX_RECEIPT_NAME {
                RECEIPT_TEST_FLIGHT
            } else {
                RECEIPT_APP_STORE
            }
        }
        _ => RECEIPT_NONE,
    }
}

pub fn is_troll_store_install(receipt: c_int, troll_store: bool, troll_store_lite: bool) -> bool {
    receipt == RECEIPT_NONE && (troll_store || troll_store_lite)
}

pub fn troll_store_variant(troll_store: bool, troll_store_lite: bool) -> &'static str {
    if troll_store {
        "TrollStore"
    } else if troll_store_lite {
        "TrollStore Lite"
    } else {
        "Unknown"
    }
}

pub struct InstallFacts {
    pub simulator: bool,
    pub receipt: c_int,
    pub troll_store: bool,
    pub troll_store_lite: bool,
    pub live_container: bool,
    pub jit: bool,
    pub discord_signed: Option<bool>,
}

pub fn app_source(facts: &InstallFacts) -> &'static str {
    if facts.simulator {
        return "iOS Simulator";
    }
    if is_troll_store_install(facts.receipt, facts.troll_store, facts.troll_store_lite) {
        return troll_store_variant(facts.troll_store, facts.troll_store_lite);
    }
    if facts.live_container {
        return if facts.jit {
            "LiveContainer (JIT)"
        } else {
            "LiveContainer (Signed)"
        };
    }
    if facts.receipt != RECEIPT_NONE && facts.discord_signed == Some(false) {
        return if facts.receipt == RECEIPT_TEST_FLIGHT {
            "Sideloaded (TestFlight build)"
        } else {
            "Sideloaded (App Store build)"
        };
    }
    match facts.receipt {
        RECEIPT_APP_STORE => "App Store",
        RECEIPT_TEST_FLIGHT => "TestFlight",
        _ => "Sideloaded",
    }
}

pub struct SideloaderMarkers<'a> {
    pub url_schemes: &'a [Vec<u8>],
    pub exported_types: &'a [Vec<u8>],
    pub alt_bundle_identifier: bool,
    pub alt_device_identifier: bool,
}

fn any_starts_with(values: &[Vec<u8>], prefix: &[u8]) -> bool {
    values.iter().any(|value| value.starts_with(prefix))
}

pub fn sideloader_name(markers: &SideloaderMarkers) -> &'static str {
    if any_starts_with(markers.url_schemes, b"sidestore-") || any_starts_with(markers.exported_types, b"io.sidestore.Installed.") {
        return "SideStore";
    }
    if any_starts_with(markers.url_schemes, b"altstore-") || any_starts_with(markers.exported_types, b"io.altstore.Installed.") {
        return "AltStore";
    }
    if markers.alt_bundle_identifier || markers.alt_device_identifier {
        "AltStore/SideStore"
    } else {
        ""
    }
}

fn replace_all(text: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(text.len());
    let mut index = 0;
    while index < text.len() {
        if text[index..].starts_with(from) {
            result.extend_from_slice(to);
            index += from.len();
        } else {
            result.push(text[index]);
            index += 1;
        }
    }
    result
}

fn certificate_team_name(name: &[u8]) -> Vec<u8> {
    const MAX_TEAM_NAME: usize = 60;
    let clean: Vec<u8> = name.iter().copied().filter(|&byte| byte != b'`' && byte >= 0x20).collect();
    let clean = replace_all(&clean, " · ".as_bytes(), b" - ");
    let start = clean.iter().position(|&byte| byte != b' ');
    let end = clean.iter().rposition(|&byte| byte != b' ');
    let mut trimmed = match (start, end) {
        (Some(start), Some(end)) => clean[start..=end].to_vec(),
        _ => Vec::new(),
    };
    trimmed.truncate(MAX_TEAM_NAME);
    trimmed
}

pub struct SigningFacts<'a> {
    pub team_identifier: Option<&'a [u8]>,
    pub get_task_allow: bool,
    pub has_profile: bool,
    pub provisions_all_devices: bool,
    pub lists_devices: bool,
    pub profile_expiry: Option<&'a [u8]>,
    pub team_name: Option<&'a [u8]>,
    pub sideloader: &'a [u8],
}

pub fn is_discord_production_team(team_identifier: Option<&[u8]>) -> bool {
    team_identifier == Some(DISCORD_TEAM_IDENTIFIER)
}

pub fn signing_details(facts: &SigningFacts) -> Vec<u8> {
    let discord = is_discord_production_team(facts.team_identifier);
    let kind: &[u8] = if discord {
        b"Distribution"
    } else if facts.provisions_all_devices {
        b"Enterprise"
    } else if facts.get_task_allow {
        b"Development"
    } else if facts.lists_devices {
        b"Ad Hoc"
    } else if facts.has_profile {
        b"Distribution"
    } else {
        b"Unprovisioned"
    };
    let mut first = kind.to_vec();
    if let Some(team) = facts.team_identifier.filter(|team| !team.is_empty()) {
        first.extend_from_slice(b" (`");
        first.extend_from_slice(team);
        first.extend_from_slice(b"`)");
    }
    let mut parts = vec![first];
    if facts.provisions_all_devices
        && !discord
        && let Some(team) = facts.team_name.map(certificate_team_name).filter(|team| !team.is_empty())
    {
        parts.push(team);
    }
    if let Some(expiry) = facts.profile_expiry.filter(|_| !discord) {
        parts.push([b"exp ".as_slice(), expiry].concat());
    }
    if !facts.sideloader.is_empty() {
        parts.push(facts.sideloader.to_vec());
    }
    parts.join(" · ".as_bytes())
}

fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if from > haystack.len() || needle.is_empty() {
        return None;
    }
    haystack[from..]
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|index| index + from)
}

pub fn provisioning_profile_plist(profile: &[u8]) -> Option<Vec<u8>> {
    const END: &[u8] = b"</plist>";
    let start = find(profile, b"<?xml", 0)?;
    let end = find(profile, END, start)?;
    Some(profile[start..end + END.len()].to_vec())
}

pub fn is_virtual_device_machine(machine: &[u8]) -> bool {
    machine == VIRTUAL_DEVICE_MACHINE
}

pub fn os_version_label(version: &[u8], build: Option<&[u8]>) -> Vec<u8> {
    match build {
        Some(build) => [version, b" (", build, b")"].concat(),
        None => version.to_vec(),
    }
}

const ROUTE_MESSAGE_HEADER_SIZE: usize = 92;
const ROUTE_ADDRESS_MASK_OFFSET: usize = 12;
const ROUTE_ADDRESS_COUNT: usize = 8;
const SOCKET_ADDRESS_ALIGNMENT: usize = 4;
const IPV4_ADDRESS_SIZE: u8 = 16;
const IPV4_ADDRESS_OFFSET: usize = 4;
const IPV4_FAMILY: u8 = 2;

fn socket_address_length(declared: u8) -> usize {
    if declared > 0 {
        1 + ((usize::from(declared) - 1) | (SOCKET_ADDRESS_ALIGNMENT - 1))
    } else {
        SOCKET_ADDRESS_ALIGNMENT
    }
}

fn is_ipv4_address(address: Option<&[u8]>) -> bool {
    address.is_some_and(|address| address.len() >= 8 && address[1] == IPV4_FAMILY && address[0] >= IPV4_ADDRESS_SIZE)
}

fn default_route_gateway(message: &[u8]) -> Option<String> {
    let mask = i32::from_ne_bytes(message[ROUTE_ADDRESS_MASK_OFFSET..ROUTE_ADDRESS_MASK_OFFSET + 4].try_into().ok()?);
    let mut addresses: [Option<&[u8]>; ROUTE_ADDRESS_COUNT] = [None; ROUTE_ADDRESS_COUNT];
    let mut cursor = ROUTE_MESSAGE_HEADER_SIZE;
    for (index, slot) in addresses.iter_mut().enumerate() {
        if mask & (1 << index) == 0 {
            continue;
        }
        if cursor >= message.len() {
            return None;
        }
        let length = socket_address_length(message[cursor]);
        if length > message.len() - cursor {
            return None;
        }
        *slot = Some(&message[cursor..cursor + length]);
        cursor += length;
    }
    let (destination, gateway) = (addresses[0], addresses[1]);
    if !is_ipv4_address(destination) || !is_ipv4_address(gateway) {
        return None;
    }
    let destination = destination?;
    if destination[IPV4_ADDRESS_OFFSET..IPV4_ADDRESS_OFFSET + 4] != [0, 0, 0, 0] {
        return None;
    }
    let octets = &gateway?[IPV4_ADDRESS_OFFSET..IPV4_ADDRESS_OFFSET + 4];
    Some(format!("{}.{}.{}.{}", octets[0], octets[1], octets[2], octets[3]))
}

pub fn default_ipv4_gateway(table: &[u8]) -> Option<String> {
    let mut cursor = 0;
    while table.len() - cursor >= ROUTE_MESSAGE_HEADER_SIZE {
        let length = usize::from(u16::from_ne_bytes([table[cursor], table[cursor + 1]]));
        if length < ROUTE_MESSAGE_HEADER_SIZE || length > table.len() - cursor {
            break;
        }
        if let Some(gateway) = default_route_gateway(&table[cursor..cursor + length]) {
            return Some(gateway);
        }
        cursor += length;
    }
    None
}

pub fn hook_framework_for_image(image_path: &[u8], exports_libhooker_api: bool) -> Option<(&'static str, &'static str)> {
    const MARKERS: [(&[u8], (&str, &str)); 4] = [
        (b"ellekit", ("ElleKit", "ellekit")),
        (b"libhooker", ("libhooker", "org.coolstar.libhooker")),
        (b"substitute", ("Substitute", "com.ex.substitute")),
        (b"substrate", ("Substrate", "mobilesubstrate")),
    ];
    let lowered = image_path.to_ascii_lowercase();
    let (_, framework) = MARKERS.iter().find(|(marker, _)| find(&lowered, marker, 0).is_some())?;
    Some(if framework.0 == "Substrate" && exports_libhooker_api {
        MARKERS[0].1
    } else {
        *framework
    })
}

pub fn dpkg_package_version(status: &[u8], package: &[u8]) -> Option<Vec<u8>> {
    let mut in_package = false;
    for line in status.split(|&byte| byte == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            in_package = false;
        } else if let Some(name) = line.strip_prefix(b"Package: ") {
            in_package = name == package;
        } else if in_package && let Some(version) = line.strip_prefix(b"Version: ") {
            return Some(version.to_vec());
        }
    }
    None
}

pub fn framework_label(name: &[u8], version: &[u8]) -> Vec<u8> {
    if name.is_empty() || version.is_empty() {
        name.to_vec()
    } else {
        [name, b" ", version].concat()
    }
}

pub fn bootstrap_root_of_image(image_path: &[u8]) -> Option<Vec<u8>> {
    const ROOTHIDE: &[u8] = b"/.jbroot-";
    if let Some(start) = find(image_path, ROOTHIDE, 0) {
        let end = find(image_path, b"/", start + ROOTHIDE.len()).unwrap_or(image_path.len());
        return Some(image_path[..end].to_vec());
    }
    const INJECTION_FOLDERS: [&[u8]; 6] = [
        b"/Library/MobileSubstrate/DynamicLibraries/",
        b"/usr/lib/TweakInject/",
        b"/usr/lib/ellekit/",
        b"/usr/lib/TweakLoader.dylib",
        b"/usr/lib/libellekit.dylib",
        b"/usr/lib/libsubstrate.dylib",
    ];
    INJECTION_FOLDERS
        .iter()
        .find_map(|folder| find(image_path, folder, 0).filter(|&found| found > 0))
        .map(|found| image_path[..found].to_vec())
}

pub fn jailbreak_name(root: &[u8], markers: &[Vec<u8>], images: &[Vec<u8>], roothide: bool) -> String {
    if root.is_empty() {
        return String::new();
    }
    let hidden = roothide || find(root, b"/.jbroot-", 0).is_some();
    let has = |marker: &[u8]| markers.iter().any(|entry| entry == marker);
    if has(b".installed_relaxin") {
        return "Relaxin".into();
    }
    let prefix = [root, b"/"].concat();
    if images
        .iter()
        .any(|image| image.starts_with(&prefix) && (find(image, b"/relaxin", 0).is_some() || find(image, b"/librelaxin", 0).is_some()))
    {
        return "Relaxin".into();
    }
    if has(b".xia0o0o0o_jb_installed") {
        return if hidden { "roothide".into() } else { String::new() };
    }
    const NAMES: [(&[u8], &str); 6] = [
        (b".installed_nekojb", "nekoJB"),
        (b".installed_nathanlr", "nathanlr"),
        (b".palecursus_strapped", "palera1n"),
        (b".installed_palera1n", "palera1n"),
        (b".installed_dopamine", "Dopamine"),
        (b"basebins", "Serotonin"),
    ];
    if let Some((_, name)) = NAMES.iter().find(|(marker, _)| has(marker)) {
        return if hidden { format!("{name} (roothide)") } else { (*name).into() };
    }
    if hidden { "roothide".into() } else { String::new() }
}

pub fn jailbreak_label(name: &[u8], version: &[u8]) -> Vec<u8> {
    if name != b"Dopamine" || version.is_empty() {
        return name.to_vec();
    }
    let (mut previous_dot, mut parts) = (true, 1);
    for &character in version {
        if character == b'.' {
            parts += 1;
            if previous_dot || parts > 4 {
                return name.to_vec();
            }
            previous_dot = true;
        } else if character.is_ascii_digit() {
            previous_dot = false;
        } else {
            return name.to_vec();
        }
    }
    if previous_dot || parts < 2 {
        name.to_vec()
    } else {
        [name, b" ", version].concat()
    }
}

fn present<'a>(data: *const c_char, length: usize) -> Option<&'a [u8]> {
    (!data.is_null()).then(|| bytes(data, length))
}

fn list(data: *const c_char, length: usize) -> Vec<Vec<u8>> {
    let joined = bytes(data, length);
    if joined.is_empty() {
        return Vec::new();
    }
    joined
        .strip_suffix(&[0])
        .unwrap_or(joined)
        .split(|&byte| byte == 0)
        .map(<[u8]>::to_vec)
        .collect()
}

fn give(value: impl Into<Vec<u8>>) -> AstralRustString {
    AstralRustString::new(value.into())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_environment_receipt_kind(name: *const c_char, length: usize, exists: c_int) -> c_int {
    receipt_kind(present(name, length), exists != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_environment_is_troll_store_install(receipt: c_int, troll_store: c_int, troll_store_lite: c_int) -> c_int {
    c_int::from(is_troll_store_install(receipt, troll_store != 0, troll_store_lite != 0))
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_environment_troll_store_variant(troll_store: c_int, troll_store_lite: c_int) -> AstralRustString {
    give(troll_store_variant(troll_store != 0, troll_store_lite != 0))
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_environment_app_source(
    simulator: c_int,
    receipt: c_int,
    troll_store: c_int,
    troll_store_lite: c_int,
    live_container: c_int,
    jit: c_int,
    discord_signed: c_int,
) -> AstralRustString {
    give(app_source(&InstallFacts {
        simulator: simulator != 0,
        receipt,
        troll_store: troll_store != 0,
        troll_store_lite: troll_store_lite != 0,
        live_container: live_container != 0,
        jit: jit != 0,
        discord_signed: (discord_signed >= 0).then_some(discord_signed != 0),
    }))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_environment_sideloader_name(
    url_schemes: *const c_char,
    url_schemes_length: usize,
    exported_types: *const c_char,
    exported_types_length: usize,
    alt_bundle_identifier: c_int,
    alt_device_identifier: c_int,
) -> AstralRustString {
    let (url_schemes, exported_types) = (list(url_schemes, url_schemes_length), list(exported_types, exported_types_length));
    give(sideloader_name(&SideloaderMarkers {
        url_schemes: &url_schemes,
        exported_types: &exported_types,
        alt_bundle_identifier: alt_bundle_identifier != 0,
        alt_device_identifier: alt_device_identifier != 0,
    }))
}

#[repr(C)]
pub struct AstralSigningFacts {
    pub team_identifier: *const c_char,
    pub team_identifier_length: usize,
    pub get_task_allow: c_int,
    pub has_profile: c_int,
    pub provisions_all_devices: c_int,
    pub lists_devices: c_int,
    pub profile_expiry: *const c_char,
    pub profile_expiry_length: usize,
    pub team_name: *const c_char,
    pub team_name_length: usize,
    pub sideloader: *const c_char,
    pub sideloader_length: usize,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_environment_signing_details(facts: *const AstralSigningFacts) -> AstralRustString {
    let Some(facts) = (unsafe { facts.as_ref() }) else {
        return give(Vec::new());
    };
    give(signing_details(&SigningFacts {
        team_identifier: present(facts.team_identifier, facts.team_identifier_length),
        get_task_allow: facts.get_task_allow != 0,
        has_profile: facts.has_profile != 0,
        provisions_all_devices: facts.provisions_all_devices != 0,
        lists_devices: facts.lists_devices != 0,
        profile_expiry: present(facts.profile_expiry, facts.profile_expiry_length),
        team_name: present(facts.team_name, facts.team_name_length),
        sideloader: bytes(facts.sideloader, facts.sideloader_length),
    }))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_environment_provisioning_profile_plist(profile: *const c_char, length: usize) -> AstralRustString {
    optional(provisioning_profile_plist(bytes(profile, length)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_environment_is_discord_production_team(team: *const c_char, length: usize) -> c_int {
    c_int::from(is_discord_production_team(present(team, length)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_environment_is_virtual_device_machine(machine: *const c_char, length: usize) -> c_int {
    c_int::from(is_virtual_device_machine(bytes(machine, length)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_environment_os_version_label(
    version: *const c_char,
    version_length: usize,
    build: *const c_char,
    build_length: usize,
) -> AstralRustString {
    give(os_version_label(bytes(version, version_length), present(build, build_length)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_environment_default_ipv4_gateway(table: *const u8, length: usize) -> AstralRustString {
    if table.is_null() {
        return optional(None);
    }
    let table = unsafe { core::slice::from_raw_parts(table, length) };
    optional(default_ipv4_gateway(table).map(String::into_bytes))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_environment_hook_framework_for_image(
    path: *const c_char,
    length: usize,
    libhooker: c_int,
) -> AstralRustString {
    match hook_framework_for_image(bytes(path, length), libhooker != 0) {
        Some((name, package)) => AstralRustString::frames(&[name.as_bytes().to_vec(), package.as_bytes().to_vec()]),
        None => optional(None),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_environment_dpkg_package_version(
    status: *const c_char,
    status_length: usize,
    package: *const c_char,
    package_length: usize,
) -> AstralRustString {
    optional(dpkg_package_version(bytes(status, status_length), bytes(package, package_length)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_environment_framework_label(
    name: *const c_char,
    name_length: usize,
    version: *const c_char,
    version_length: usize,
) -> AstralRustString {
    give(framework_label(bytes(name, name_length), bytes(version, version_length)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_environment_jailbreak_name(
    root: *const c_char,
    root_length: usize,
    markers: *const c_char,
    markers_length: usize,
    images: *const c_char,
    images_length: usize,
    roothide: c_int,
) -> AstralRustString {
    give(jailbreak_name(
        bytes(root, root_length),
        &list(markers, markers_length),
        &list(images, images_length),
        roothide != 0,
    ))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_environment_jailbreak_label(
    name: *const c_char,
    name_length: usize,
    version: *const c_char,
    version_length: usize,
) -> AstralRustString {
    give(jailbreak_label(bytes(name, name_length), bytes(version, version_length)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_environment_bootstrap_root_of_image(path: *const c_char, length: usize) -> AstralRustString {
    optional(bootstrap_root_of_image(bytes(path, length)))
}
