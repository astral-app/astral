use astral_core::diagnostics::locate::{AstralImageView, astral_locate_callers, astral_locate_const_referencing, astral_locate_pointer_to};
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
    assert!(helper != 0, "gate helper not located");
    let mut evaluators = [0u64; 4];
    let count = unsafe { astral_locate_callers(&view, helper, evaluators.as_mut_ptr(), 4) };
    println!("helper={helper:#x} evaluators={:x?}", &evaluators[..count]);
    assert!(count >= 1, "no evaluators located");
    for evaluator in &evaluators[..count] {
        let mut slots = [0u64; 8];
        let n = unsafe { astral_locate_pointer_to(&view, *evaluator, slots.as_mut_ptr(), 8) };
        println!("  evaluator {evaluator:#x}: {n} dispatch pointers (file mode: fixups unresolved, expect 0)");
    }
}
