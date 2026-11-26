use astral_tools::lua_casing::findings;

#[test]
fn flags_camel_case_and_allows_classes() {
    let source = "local fooBar = 1\nlocal CONSTANT = 2\nlocal Thing = {}\nThing.__index = Thing\nfunction module.doThing(someArg) end\nfor badName in pairs({}) do end\nlocal text = \"camelCase inside a string\"\n";
    let names: Vec<(String, &str)> = findings(source)
        .into_iter()
        .map(|finding| (finding.name, finding.kind))
        .collect();
    assert_eq!(
        names,
        vec![
            ("fooBar".to_owned(), "local"),
            ("doThing".to_owned(), "function"),
            ("someArg".to_owned(), "parameter"),
            ("badName".to_owned(), "loop variable"),
        ]
    );
}
