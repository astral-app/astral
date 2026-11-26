use std::collections::BTreeMap;
use std::path::PathBuf;

use astral_tools::signing;
use ed25519_dalek::SigningKey;

fn write_plugin(name: &str) -> PathBuf {
    let folder = std::env::temp_dir().join(format!("astral-tools-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    for (path, contents) in [
        (
            "manifest.toml",
            "id = \"astral.sample\"\nname = \"Sample\"\nmain = \"main.lua\"\npermissions = [\"storage\"]\n",
        ),
        ("main.lua", "local util = require(\"lib.util\")\n"),
        ("lib/util.lua", "return {}\n"),
        ("i18n/en-US.toml", "HELLO = \"Hello\"\n"),
        ("tests/sample.lua", "print(\"ok\")\n"),
    ] {
        let target = folder.join(path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(target, contents).unwrap();
    }
    folder
}

#[test]
fn signed_paths_cover_modules_but_not_tests() {
    let paths = signing::signed_paths(&write_plugin("paths")).unwrap();
    assert_eq!(
        paths.join(","),
        "i18n/en-US.toml,lib/util.lua,lib/util.luac,main.lua,main.luac,manifest.toml"
    );
}

#[test]
fn compile_plugin_writes_bytecode_with_chunk_names() {
    let folder = write_plugin("compile");
    signing::compile_plugin(&folder).unwrap();
    for path in ["main.luac", "lib/util.luac"] {
        let data = std::fs::read(folder.join(path)).unwrap();
        assert!(data.starts_with(b"\x1bLua"), "{path} is not Lua bytecode");
    }
    let util = std::fs::read(folder.join("lib/util.luac")).unwrap();
    assert!(
        util.windows(22)
            .any(|window| window == b"astral.sample/lib.util")
    );
    assert!(!folder.join("tests/sample.luac").exists());
}

#[test]
fn signature_round_trip_and_tamper() {
    let key = SigningKey::from_bytes(&[7; 32]);
    let public = key.verifying_key();
    let folder = write_plugin("tamper");
    signing::write_plugin_signature(&folder, &key).unwrap();
    signing::verify_plugin(&folder, &public).unwrap();
    std::fs::write(folder.join("lib/util.lua"), "return { changed = true }\n").unwrap();
    assert!(signing::verify_plugin(&folder, &public).is_err());
    signing::write_plugin_signature(&folder, &key).unwrap();
    std::fs::write(folder.join("lib/extra.lua"), "return 1\n").unwrap();
    assert!(signing::verify_plugin(&folder, &public).is_err());
    std::fs::remove_file(folder.join("lib/extra.lua")).unwrap();
    let other = SigningKey::from_bytes(&[9; 32]).verifying_key();
    assert!(signing::verify_plugin(&folder, &other).is_err());
}

#[test]
fn signature_message_is_sorted() {
    let files = BTreeMap::from([
        ("main.lua".to_owned(), "b".to_owned()),
        ("manifest.toml".to_owned(), "a".to_owned()),
        ("i18n/x.toml".to_owned(), "c".to_owned()),
    ]);
    assert_eq!(
        String::from_utf8(signing::signature_message(&files)).unwrap(),
        "astral-plugin-v2\ni18n/x.toml c\nmain.lua b\nmanifest.toml a\n"
    );
}
