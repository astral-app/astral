use astral_tools::objc_boxing::findings;

#[test]
fn flags_boxed_comparisons_and_spares_casts() {
    let source = "a = @(x && y);\nb = @((BOOL)(x && y));\nc = @(count);\nd = @(!flag);\ne = @(value == 2 ? 1 : 0);\nf = @([items count] > 0 || ready);\n";
    let lines: Vec<usize> = findings(source)
        .iter()
        .map(|finding| finding.line)
        .collect();
    assert_eq!(lines, vec![1, 4, 6]);
}
