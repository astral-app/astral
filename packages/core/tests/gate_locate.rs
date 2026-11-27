use astral_core::diagnostics::locate::{
    AstralImageView, astral_locate_callers, astral_locate_const_referencing, astral_locate_phoenix_access_gate,
};
use std::ffi::CString;

#[test]
fn locates_phoenix_access_gate() {
    let Ok(path) = std::env::var("ASTRAL_PHOENIX_BINARY") else {
        return;
    };
    let bytes = std::fs::read(&path).expect("read phoenix binary");
    let view = AstralImageView {
        image: bytes.as_ptr(),
        length: bytes.len(),
        slide: 0,
        file: 1,
    };
    let needle = CString::new("2026-11-phoenix-app-access").unwrap();
    let helper = unsafe { astral_locate_const_referencing(&view, needle.as_ptr()) };
    let mut evaluators = [0u64; 4];
    let count = unsafe { astral_locate_phoenix_access_gate(&view, evaluators.as_mut_ptr(), 4) };
    assert_eq!(count, 2, "gate must resolve to exactly two evaluators");
    let mut helper_callers = [0u64; 4];
    let helper_count = unsafe { astral_locate_callers(&view, helper, helper_callers.as_mut_ptr(), 4) };
    assert!(
        evaluators[..count]
            .iter()
            .all(|entry| !helper_callers[..helper_count].contains(entry)),
        "experiment notification callers are not access evaluators"
    );
    println!("evaluators={:x?}", &evaluators[..count]);
}
