use astral_tools::lua_enums;

#[test]
fn flags_string_literals_that_have_enums() {
    let source = "astral.events.on(\"gateway\", handler)\nlocal node = { type = \"view\", justify = \"center\" }\nif media.type == \"video\" then end\n";
    let found = lua_enums::findings(source);
    let lines: Vec<usize> = found.iter().map(|finding| finding.line).collect();
    assert_eq!(lines, vec![1, 2, 2, 3]);
    assert_eq!(
        found[0].suggestions,
        vec!["astral.Event.Gateway".to_owned()]
    );
    assert!(
        found[1]
            .suggestions
            .contains(&"astral.InsertNodeType.View".to_owned())
    );
}

#[test]
fn allows_values_without_enums() {
    let source = "local embed = { type = \"gifv\" }\nvalues.style = \"pill\"\nlocal node = { type = astral.InsertNodeType.View }\n";
    assert!(lua_enums::findings(source).is_empty());
}

#[test]
fn sdk_enums_match_the_core() {
    let path = astral_tools::repo::path(&["packages", "sdk", "library", "astral.lua"]);
    let source = astral_tools::repo::read(&path).expect("sdk");
    assert_eq!(lua_enums::rendered_sdk(&source).expect("rendered"), source);
}
