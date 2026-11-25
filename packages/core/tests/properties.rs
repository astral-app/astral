use core::ffi::c_char;

use astral_core::wire::properties::{FALLBACK, NAMES, astral_wire_reset_properties, astral_wire_resolve_properties, current};

fn names(list: &[&std::ffi::CStr]) -> Vec<*const c_char> {
    list.iter().map(|name| name.as_ptr()).collect()
}

#[test]
fn resolves_every_property_or_keeps_the_fallback() {
    let owned: Vec<std::ffi::CString> = NAMES.iter().rev().map(|name| std::ffi::CString::new(*name).unwrap()).collect();
    let pointers: Vec<*const c_char> = owned.iter().map(|name| name.as_ptr()).collect();
    let mut missing: *const c_char = core::ptr::null();
    let found = unsafe { astral_wire_resolve_properties(pointers.as_ptr(), pointers.len(), &mut missing) };
    assert_eq!(found, NAMES.len());
    assert!(missing.is_null());
    assert_eq!(
        current().angle as usize,
        NAMES.len() - 1 - NAMES.iter().position(|name| *name == "angle").unwrap()
    );

    let partial = names(&[c"angle"]);
    let mut missing: *const c_char = core::ptr::null();
    let found = unsafe { astral_wire_resolve_properties(partial.as_ptr(), partial.len(), &mut missing) };
    assert_eq!(found, 1);
    assert!(!missing.is_null());
    assert_ne!(current().angle, 0);

    astral_wire_reset_properties();
    assert_eq!(current().angle, FALLBACK.angle);
}
