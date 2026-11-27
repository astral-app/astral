use astral_tools::phoenix::{opens_android_access_gate, patch_android_access_gate};

#[test]
fn patches_the_android_access_gate() {
    let Ok(path) = std::env::var("ASTRAL_PHOENIX_SO") else {
        return;
    };
    let data = std::fs::read(&path).expect("read phoenix so");
    assert!(opens_android_access_gate(&data), "gate not located");
    let patched = patch_android_access_gate(data.clone()).expect("patch");
    assert_eq!(patched.len(), data.len());
    let return_true = [0x20u8, 0x00, 0x80, 0x52, 0xC0, 0x03, 0x5F, 0xD6];
    let changed = patched
        .windows(8)
        .zip(data.windows(8))
        .filter(|(after, before)| after != before && *after == return_true)
        .count();
    println!("patched {changed} evaluator entries");
    assert_eq!(changed, 2);
    assert_eq!(
        patch_android_access_gate(patched.clone()).expect("patch again"),
        patched
    );
}
