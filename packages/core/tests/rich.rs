use astral_core::wire::out::Out;
use astral_core::wire::rich::put_tree;
use serde_json::json;

#[test]
fn draws_a_tree_with_runs_corners_and_taps() {
    let tree = json!({
        "type": "view",
        "radius": [6, 6, 0, 0],
        "children": [
            { "type": "text", "runs": [{ "text": "local", "color": 1, "bold": true }, { "text": " x" }], "font": "monospace", "grow": true },
            { "type": "text", "text": "Copy", "tap": "astral.tap.1:copy:2.0" },
            { "type": "image", "icon": "https://cdn.discordapp.com/x.png", "cover": true }
        ]
    });
    let mut out = Out::default();
    let mut created = Vec::new();
    let root = put_tree(&mut out, &mut created, &tree);
    assert_eq!(root, created.first().copied());
    assert_eq!(created.len(), 4);
    assert_eq!(out.failed, 0);
    out.discard();
}

#[test]
fn wraps_a_list_of_nodes_and_rejects_scalars() {
    let mut out = Out::default();
    let mut created = Vec::new();
    assert!(
        put_tree(
            &mut out,
            &mut created,
            &json!([{ "type": "text", "text": "a" }, { "type": "text", "text": "b" }])
        )
        .is_some()
    );
    assert_eq!(created.len(), 3);
    assert!(put_tree(&mut out, &mut Vec::new(), &json!("nope")).is_none());
    out.discard();
}
