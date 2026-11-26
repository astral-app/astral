use astral_core::support::json_bounds::{nests_too_deeply, with_valid_utf8};

#[test]
fn replaces_each_invalid_byte() {
    assert_eq!(with_valid_utf8(b"ok"), b"ok");
    assert_eq!(with_valid_utf8(b"a\xFFb"), "a\u{FFFD}b".as_bytes());
    assert_eq!(with_valid_utf8("한".as_bytes()), "한".as_bytes());
    assert_eq!(with_valid_utf8(b"\xE2\x82"), "\u{FFFD}\u{FFFD}".as_bytes());
    assert_eq!(with_valid_utf8(b"\xED\xA0\x80"), "\u{FFFD}\u{FFFD}\u{FFFD}".as_bytes());
}

#[test]
fn counts_structural_depth_outside_strings() {
    assert!(!nests_too_deeply(b"[[[]]]", 3));
    assert!(nests_too_deeply(b"[[[[]]]]", 3));
    assert!(!nests_too_deeply(br#"["[[[[[[", {"k": "]]]"}]"#, 3));
    assert!(!nests_too_deeply(br#"["\"[[[[["]"#, 2));
}
