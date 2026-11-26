use astral_tools::ipa_archive::{Action, Ipa, rewrite};
use astral_tools::phoenix::{opens_android_access_gate, patch_android_access_gate};
#[allow(unused_imports)]
#[test]
fn patches_the_so_inside_the_apk() {
    let Ok(path) = std::env::var("ASTRAL_PHOENIX_APK") else {
        return;
    };
    let work = std::env::temp_dir().join("astral-phoenix-apk-test.apk");
    std::fs::copy(&path, &work).expect("copy apk");
    let mut archive = Ipa::open(&work).expect("open");
    let entry = archive
        .names()
        .into_iter()
        .find(|name| name.ends_with("lib/arm64-v8a/liblibdiscord_wasm_host.so"))
        .expect("so entry");
    assert!(
        opens_android_access_gate(&archive.read(&entry).expect("read")),
        "gate not located"
    );
    let target = entry.clone();
    rewrite(
        &work,
        "phoenix-access-test",
        |name| {
            if name == target {
                Action::Patch(Box::new(patch_android_access_gate))
            } else {
                Action::Keep
            }
        },
        Vec::new(),
    )
    .expect("rewrite");
    let original = Ipa::open(std::path::Path::new(&path))
        .expect("reopen source")
        .read(&entry)
        .expect("read source");
    let patched = Ipa::open(&work)
        .expect("reopen")
        .read(&entry)
        .expect("read patched");
    assert_eq!(patched.len(), original.len());
    let changes = patched
        .iter()
        .zip(&original)
        .filter(|(after, before)| after != before)
        .count();
    println!("bytes changed in the .so inside the APK: {changes}");
    assert!(changes > 0, "the APK .so was not patched");
    let again = patch_android_access_gate(patched.clone()).expect("re-patch");
    assert_eq!(again, patched, "patching is not idempotent");
    let _ = std::fs::remove_file(&work);
}
