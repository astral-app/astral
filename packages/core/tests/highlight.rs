use astral_core::highlight::{find_syntax, highlight};

const THEME: &str = r##"{"type":"dark","colors":{"editor.background":"#1E1E1E","editor.foreground":"#D4D4D4"},"tokenColors":[{"scope":["keyword","keyword.control"],"settings":{"foreground":"#C586C0","fontStyle":"bold"}},{"scope":"string","settings":{"foreground":"#CE9178"}}]}"##;

#[test]
fn highlights_lua_with_a_vscode_theme() {
    let started = std::time::Instant::now();
    let result = highlight(
        "local greeting = \"hi\"\nfor i = 1, 3 do print(greeting, i) end",
        "lua",
        THEME.as_bytes(),
    );
    eprintln!("first highlight {:?}", started.elapsed());
    assert_eq!(result["found"], true);
    assert_eq!(result["background"], 0xFF1E_1E1E_u32);
    let lines = result["lines"].as_array().unwrap();
    assert_eq!(lines.len(), 2);
    let first = &lines[1][0];
    assert_eq!(first[0], "for");
    assert_eq!(first[1], 0xFFC5_86C0_u32);
    assert_eq!(first[2], 1);
    assert!(
        lines[0]
            .as_array()
            .unwrap()
            .iter()
            .any(|run| run[0] == "hi" && run[1] == 0xFFCE_9178_u32)
    );
    let text: String = lines[1].as_array().unwrap().iter().map(|run| run[0].as_str().unwrap()).collect();
    assert_eq!(text, "for i = 1, 3 do print(greeting, i) end");
}

#[test]
fn finds_common_fence_languages() {
    for language in [
        "ts",
        "typescript",
        "rs",
        "py",
        "js",
        "json",
        "toml",
        "kotlin",
        "swift",
        "go",
        "c",
        "cpp",
        "sh",
        "yaml",
        "diff",
        "Lua",
    ] {
        assert!(find_syntax(language).is_some(), "{language}");
    }
    let plain = highlight("whatever", "nonsense", b"{}");
    assert_eq!(plain["found"], false);
    assert_eq!(plain["lines"][0][0][0], "whatever");
}
