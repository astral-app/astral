use core::ffi::{CStr, c_char};
use core::ptr::{addr_of, addr_of_mut};

include!(concat!(env!("OUT_DIR"), "/properties.rs"));

#[unsafe(no_mangle)]
pub static mut gAstralWireProperties: AstralWireProperties = FALLBACK;

pub fn current() -> AstralWireProperties {
    unsafe { addr_of!(gAstralWireProperties).read() }
}

fn replace(properties: AstralWireProperties) {
    unsafe { addr_of_mut!(gAstralWireProperties).write(properties) };
}

fn position(names: &[*const c_char], wanted: &str) -> Option<u16> {
    names
        .iter()
        .take(usize::from(u16::MAX) + 1)
        .position(|name| !name.is_null() && unsafe { CStr::from_ptr(*name) }.to_bytes() == wanted.as_bytes())
        .map(|index| index as u16)
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_wire_reset_properties() {
    replace(FALLBACK);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_wire_resolve_properties(names: *const *const c_char, count: usize, missing: *mut *const c_char) -> usize {
    let names = if names.is_null() || count == 0 {
        &[][..]
    } else {
        unsafe { core::slice::from_raw_parts(names, count) }
    };
    let mut resolved = FALLBACK;
    let mut found = 0;
    let mut first_missing: Option<&'static str> = None;
    for (index, name) in NAMES.iter().enumerate() {
        match position(names, name) {
            Some(value) => {
                resolved.set(index, value);
                found += 1;
            }
            None => first_missing = first_missing.or(Some(name)),
        }
    }
    if missing.is_null() {
        replace(resolved);
        return found;
    }
    if let Some(name) = first_missing
        && unsafe { (*missing).is_null() }
    {
        unsafe { *missing = missing_name(name) };
    }
    if unsafe { !(*missing).is_null() } {
        return found;
    }
    replace(resolved);
    found
}

fn missing_name(name: &'static str) -> *const c_char {
    static NAMES_C: std::sync::OnceLock<Vec<std::ffi::CString>> = std::sync::OnceLock::new();
    let names = NAMES_C.get_or_init(|| {
        NAMES
            .iter()
            .map(|name| std::ffi::CString::new(*name).expect("property name"))
            .collect()
    });
    NAMES
        .iter()
        .position(|candidate| *candidate == name)
        .map_or(core::ptr::null(), |index| names[index].as_ptr())
}
