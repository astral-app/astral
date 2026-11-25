use astral_core::crypto::{hex, sha256};
use astral_core::files::{is_contained_relative, join_path, list_directory, resolve_inside, write_atomically};
use astral_core::privacy::sanitize;
use astral_core::session::plausible;
use astral_core::strings::{set_strings, text};
use astral_core::toml_json::parse;

#[test]
fn hashes_match_known_digests() {
    assert_eq!(
        hex(&sha256(b"")),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        hex(&sha256(b"abc")),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(hex(&sha256(&[b'a'; 1000])).len(), 64);
}

#[test]
fn paths_stay_inside() {
    assert_eq!(join_path(b"/a", b"b"), b"/a/b");
    assert_eq!(join_path(b"/a/", b"b"), b"/a/b");
    assert_eq!(join_path(b"/a", b"/b"), b"/b");
    assert!(is_contained_relative(b"a/b"));
    assert!(!is_contained_relative(b"a/../b"));
    assert!(!is_contained_relative(b"./a"));
    assert!(!is_contained_relative(b"/a"));
    let directory = std::env::temp_dir().join(format!("astral-files-{}", std::process::id()));
    let root = directory.to_string_lossy().into_owned();
    assert!(write_atomically(&directory.join("nested/file.txt"), b"hello"));
    assert_eq!(std::fs::read(directory.join("nested/file.txt")).unwrap(), b"hello");
    assert_eq!(list_directory(&directory), vec![b"nested".to_vec()]);
    assert!(resolve_inside(root.as_bytes(), b"nested/file.txt").is_some());
    assert!(resolve_inside(root.as_bytes(), b"nested/../../escape").is_none());
    assert!(resolve_inside(root.as_bytes(), b"../escape").is_none());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn diagnostics_drop_secrets() {
    let cleaned = sanitize(b"token: abc123 at /var/mobile/Containers/x https://a.b/c?d=e");
    assert_eq!(cleaned, "token: [redacted] at [private path] https://a.b/c?[redacted]");
}

#[test]
fn strings_substitute_values() {
    set_strings(br#"{"greeting":"hi {name}, {name}"}"#);
    assert_eq!(text(b"greeting", b"hello", &[(b"name".to_vec(), b"ada".to_vec())]), b"hi ada, ada");
    assert_eq!(text(b"missing", b"fallback {x}", &[]), b"fallback {x}");
}

#[test]
fn toml_becomes_json() {
    let value = parse("name = \"x\"\n[table]\ncount = 3\nratio = 1.5\nlist = [true]\n").unwrap();
    assert_eq!(value.to_string(), r#"{"name":"x","table":{"count":3,"list":[true],"ratio":1.5}}"#);
    assert!(parse("= broken").is_none());
}

#[test]
fn session_tokens_need_three_parts() {
    assert!(plausible(b"aaaaaaaaaaaaaaaaaaaaaaaa.bbbbbb.cccccccccccccccccccccccccc"));
    assert!(!plausible(b"short.bbbbbb.cccccccccccccccccccccccccc"));
    assert!(!plausible(b"aaaaaaaaaaaaaaaaaaaaaaaa.bbbbbb.cccccccccccccccccccccccccc.d"));
}
