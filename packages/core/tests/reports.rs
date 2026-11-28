use astral_core::diagnostics::reports::{add, issue_url, list, mark_sent, plugin_failure, process_exit, text, unexpected_exit};
use serde_json::json;

#[test]
fn reports_dedupe_cap_and_render() {
    let root = std::env::temp_dir().join(format!("astral-reports-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let directory = root.to_string_lossy().into_owned();
    unsafe { astral_core::diagnostics::reports::astral_reports_configure(directory.as_ptr().cast(), directory.len()) };
    std::fs::write(
        root.join("reports.json"),
        br#"[{"kind":"weird"},{"error":"token: secret123","title":"Old"}]"#,
    )
    .unwrap();
    let loaded = list();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0]["detail"], "token: [redacted]");
    let environment = json!({"platform": "ios", "device": "iPhone", "plugins": ["astral.a"]});
    assert!(plugin_failure("astral.a", "A", "boom", &environment.clone()));
    assert!(!plugin_failure("astral.a", "A", "boom", &environment.clone()));
    assert!(unexpected_exit("astral.b", environment.clone(), json!(["astral.b"])));
    assert!(process_exit("hang", 100, 2, "stuck", environment.clone(), "astral.c", None));
    assert!(!process_exit("hang", 100, 2, "stuck", environment.clone(), "astral.c", None));
    assert!(!process_exit("other", 100, 2, "", environment.clone(), "", None));
    let reports = list();
    assert_eq!(reports[0]["title"], "Discord stopped responding while astral.c was running");
    assert_eq!(reports[1]["environment"]["plugins"], json!(["astral.b"]));
    assert_eq!(reports[2]["title"], "A stopped after errors");
    for index in 0..30 {
        add(&json!({"kind": "crash", "title": format!("crash {index}"), "detail": "x"}));
    }
    assert_eq!(list().len(), 20);
    let rendered = text(&reports[2]);
    assert!(rendered.starts_with("A stopped after errors\n\n| | |"));
    assert!(rendered.contains("| Device | iPhone |"));
    let url = issue_url(&reports[2]);
    assert!(url.starts_with("https://github.com/astral-app/astral/issues/new?title=A%20stopped%20after%20errors&body="));
    assert!(url.ends_with("&labels=plugin"));
    let fingerprint = list()[0]["fingerprint"].as_str().unwrap().to_owned();
    assert!(mark_sent(
        &fingerprint,
        Some(&json!({"number": 12, "url": "https://evil.example/12"}))
    ));
    let sent = list()
        .into_iter()
        .find(|report| report["fingerprint"] == fingerprint.as_str())
        .unwrap();
    assert_eq!(sent["sent"], true);
    assert!(sent.get("issue").is_none());
    assert!(mark_sent(
        &fingerprint,
        Some(&json!({"number": 12, "url": "https://github.com/astral-app/astral/issues/12"}))
    ));
    let linked = list()
        .into_iter()
        .find(|report| report["fingerprint"] == fingerprint.as_str())
        .unwrap();
    assert_eq!(linked["issue"]["number"], 12);
    std::fs::remove_dir_all(root).unwrap();
}
