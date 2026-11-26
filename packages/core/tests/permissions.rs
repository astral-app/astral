use std::collections::BTreeSet;

fn source(name: &str) -> String {
    std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(name)).unwrap()
}

fn quoted_after(text: &str, marker: &str) -> BTreeSet<String> {
    text.match_indices(marker)
        .filter_map(|(index, _)| {
            let rest = &text[index + marker.len()..];
            let rest = rest.strip_prefix('"')?;
            let end = rest.find('"')?;
            Some(rest[..end].to_owned()).filter(|name| name.bytes().all(|byte| byte.is_ascii_lowercase() || byte == b'.' || byte == b'_'))
        })
        .collect()
}

#[test]
fn every_required_permission_has_a_description() {
    let pages = source("settings_pages.rs");
    let described = quoted_after(&pages, "\n        ");
    let mut required = BTreeSet::new();
    for file in ["lua_host_api.rs", "lua_host_ui.rs", "lua_host_http.rs"] {
        required.extend(quoted_after(&source(file), "require_permission(state, "));
        required.extend(quoted_after(&source(file), "has_permission(state, "));
    }
    required.extend(quoted_after(&source("lua_host_ui.rs"), "=> Some("));
    assert!(required.len() > 20, "found too few permission checks: {required:?}");
    let missing: Vec<&String> = required.iter().filter(|permission| !described.contains(*permission)).collect();
    assert!(
        missing.is_empty(),
        "permissions without a description in settings_pages.rs: {missing:?}"
    );
}
