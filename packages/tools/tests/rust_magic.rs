use astral_tools::rust_magic;

#[test]
fn flags_unnamed_numbers_in_comparisons_and_arms() {
    let source = "fn status(code: u16) -> bool {\n    code == 200\n}\nfn kind(tag: u8) {\n    match tag {\n        6 => {}\n        _ => {}\n    }\n}\n";
    let lines: Vec<usize> = rust_magic::findings(source)
        .iter()
        .map(|finding| finding.line)
        .collect();
    assert_eq!(lines, vec![2, 6]);
}

#[test]
fn allows_constants_decoders_and_small_numbers() {
    let source = "const OK: u16 = 200;\nfn parse(byte: u8) -> u8 {\n    match byte {\n        6 => 1,\n        _ => 0,\n    }\n}\nfn check(count: usize) -> bool {\n    count == 2 || count == OK as usize\n}\n";
    assert!(rust_magic::findings(source).is_empty());
}
