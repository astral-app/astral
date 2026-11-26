use core::ffi::{CStr, c_char, c_int, c_uint, c_void};
use std::net::Ipv4Addr;

use serde_json::{Map, Value, json};

const CODE_SIGNING_STATUS: c_uint = 0;
const CODE_SIGNING_ENTITLEMENTS_BLOB: c_uint = 7;
const CODE_SIGNING_DEBUGGED: u32 = 0x1000_0000;
const BLOB_HEADER_LENGTH: usize = 8;
const MAX_BLOB_LENGTH: usize = 1 << 20;
const PROCESS_TRACED: c_int = 0x0000_0800;
const KINFO_PROC_SIZE: usize = 648;
const PROCESS_FLAG_OFFSET: usize = 32;
const SYSCTL_TEXT_LENGTH: usize = 64;
const DUPLICATE_VARIABLE: &CStr = c"ASTRAL_DUPLICATE_IMAGE";
const SIDESTORE_VPN_PREFIX: [u8; 3] = [10, 7, 0];
const TUNNEL_PREFIX: &[u8] = b"utun";

unsafe extern "C" {
    fn csops(pid: libc::pid_t, ops: c_uint, useraddr: *mut c_void, usersize: usize) -> c_int;
    fn _dyld_image_count() -> u32;
    fn _dyld_get_image_name(index: u32) -> *const c_char;
}

fn text_of(pointer: *const c_char) -> String {
    if pointer.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned()
}

fn sysctl_text(name: &CStr) -> String {
    let mut value = [0u8; SYSCTL_TEXT_LENGTH];
    let mut size = value.len();
    let status = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            value.as_mut_ptr().cast(),
            &raw mut size,
            core::ptr::null_mut(),
            0,
        )
    };
    if status != 0 {
        return String::new();
    }
    CStr::from_bytes_until_nul(&value)
        .map(|text| text.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn image_of(address: *const c_void) -> Option<libc::Dl_info> {
    let mut info: libc::Dl_info = unsafe { core::mem::zeroed() };
    (unsafe { libc::dladdr(address, &raw mut info) } != 0 && !info.dli_fname.is_null())
        .then_some(info)
}

fn image_path() -> String {
    let marker: fn() -> String = image_path;
    image_of(marker as *const c_void)
        .map(|info| text_of(info.dli_fname))
        .unwrap_or_default()
}

fn loaded_images() -> Vec<String> {
    (0..unsafe { _dyld_image_count() })
        .map(|index| text_of(unsafe { _dyld_get_image_name(index) }))
        .filter(|name| !name.is_empty())
        .collect()
}

fn symbol(name: &CStr) -> *mut c_void {
    unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) }
}

fn bootstrap_root() -> String {
    let getter = symbol(c"get_jbroot");
    if getter.is_null() {
        return String::new();
    }
    let getter: extern "C" fn() -> *const c_char = unsafe { core::mem::transmute(getter) };
    text_of(getter())
}

fn roothide_jailbroken() -> bool {
    let getter = symbol(c"jbclient_roothide_jailbroken");
    if getter.is_null() {
        return false;
    }
    let getter: extern "C" fn() -> bool = unsafe { core::mem::transmute(getter) };
    getter()
}

fn hook_provider() -> Value {
    let message_hook = symbol(c"MSHookMessageEx");
    let Some(provider) = image_of(message_hook).filter(|_| !message_hook.is_null()) else {
        return Value::Null;
    };
    let libhooker_hooks = symbol(c"LHHookFunctions");
    let same_image = !libhooker_hooks.is_null()
        && image_of(libhooker_hooks)
            .is_some_and(|libhooker| libhooker.dli_fbase == provider.dli_fbase);
    json!({"path": text_of(provider.dli_fname), "libhooker": same_image})
}

fn code_signing_debugged() -> bool {
    let mut flags: u32 = 0;
    let status = unsafe {
        csops(
            libc::getpid(),
            CODE_SIGNING_STATUS,
            (&raw mut flags).cast(),
            size_of::<u32>(),
        )
    };
    status == 0 && flags & CODE_SIGNING_DEBUGGED != 0
}

fn debugger_attached() -> bool {
    let mut info = [0u8; KINFO_PROC_SIZE];
    let mut size = info.len();
    let mut name = [
        libc::CTL_KERN,
        libc::KERN_PROC,
        libc::KERN_PROC_PID,
        unsafe { libc::getpid() },
    ];
    let status = unsafe {
        libc::sysctl(
            name.as_mut_ptr(),
            name.len() as c_uint,
            info.as_mut_ptr().cast(),
            &raw mut size,
            core::ptr::null_mut(),
            0,
        )
    };
    let flag_bytes = info[PROCESS_FLAG_OFFSET..PROCESS_FLAG_OFFSET + size_of::<c_int>()]
        .try_into()
        .unwrap_or_default();
    status == 0 && c_int::from_ne_bytes(flag_bytes) & PROCESS_TRACED != 0
}

fn entitlements_blob(length: usize) -> Option<Vec<u8>> {
    let mut blob = vec![0u8; length];
    let status = unsafe {
        csops(
            libc::getpid(),
            CODE_SIGNING_ENTITLEMENTS_BLOB,
            blob.as_mut_ptr().cast(),
            length,
        )
    };
    (status == 0).then_some(blob)
}

fn kernel_entitlements() -> String {
    let mut header = [0u8; BLOB_HEADER_LENGTH];
    let status = unsafe {
        csops(
            libc::getpid(),
            CODE_SIGNING_ENTITLEMENTS_BLOB,
            header.as_mut_ptr().cast(),
            header.len(),
        )
    };
    if status == 0 {
        return astral_core::lua::host::install::encode_base64(&header);
    }
    if std::io::Error::last_os_error().raw_os_error() != Some(libc::ERANGE) {
        return String::new();
    }
    let length = u32::from_be_bytes([header[4], header[5], header[6], header[7]]) as usize;
    if length <= BLOB_HEADER_LENGTH || length > MAX_BLOB_LENGTH {
        return String::new();
    }
    entitlements_blob(length)
        .map(|blob| astral_core::lua::host::install::encode_base64(&blob))
        .unwrap_or_default()
}

fn is_sidestore_tunnel(entry: &libc::ifaddrs) -> bool {
    if entry.ifa_addr.is_null() || entry.ifa_flags & libc::IFF_UP as c_uint == 0 {
        return false;
    }
    let name = unsafe { CStr::from_ptr(entry.ifa_name) }.to_bytes();
    let address = unsafe { &*entry.ifa_addr };
    if !name.starts_with(TUNNEL_PREFIX) || c_int::from(address.sa_family) != libc::AF_INET {
        return false;
    }
    let ipv4 = unsafe { &*entry.ifa_addr.cast::<libc::sockaddr_in>() };
    let octets = Ipv4Addr::from(u32::from_be(ipv4.sin_addr.s_addr)).octets();
    octets[..3] == SIDESTORE_VPN_PREFIX
}

fn has_sidestore_vpn() -> bool {
    let mut interfaces: *mut libc::ifaddrs = core::ptr::null_mut();
    if unsafe { libc::getifaddrs(&raw mut interfaces) } != 0 {
        return false;
    }
    let mut found = false;
    let mut entry = interfaces;
    while !entry.is_null() && !found {
        let current = unsafe { &*entry };
        found = is_sidestore_tunnel(current);
        entry = current.ifa_next;
    }
    unsafe { libc::freeifaddrs(interfaces) };
    found
}

fn duplicate_copy() -> String {
    text_of(unsafe { libc::getenv(DUPLICATE_VARIABLE.as_ptr()) })
}

fn receipt_exists(facts: &Map<String, Value>) -> bool {
    facts
        .get("receiptPath")
        .and_then(Value::as_str)
        .filter(|path| !path.is_empty())
        .is_some_and(|path| std::path::Path::new(path).exists())
}

pub fn merge(platform: &str) -> String {
    let mut facts = serde_json::from_str::<Value>(platform)
        .ok()
        .and_then(|facts| facts.as_object().cloned())
        .unwrap_or_default();
    let receipt = receipt_exists(&facts);
    facts.remove("receiptPath");
    for (key, value) in [
        ("imagePath", json!(image_path())),
        ("images", json!(loaded_images())),
        ("bootstrapRoot", json!(bootstrap_root())),
        ("roothide", json!(roothide_jailbroken())),
        ("hookProvider", hook_provider()),
        ("machine", json!(sysctl_text(c"hw.machine"))),
        ("osBuild", json!(sysctl_text(c"kern.osversion"))),
        ("codeSigningDebugged", json!(code_signing_debugged())),
        ("debuggerAttached", json!(debugger_attached())),
        ("sideStoreVPN", json!(has_sidestore_vpn())),
        ("kernelEntitlements", json!(kernel_entitlements())),
        ("receiptExists", json!(receipt)),
        ("duplicateCopy", json!(duplicate_copy())),
    ] {
        facts.insert(key.to_owned(), value);
    }
    Value::Object(facts).to_string()
}
