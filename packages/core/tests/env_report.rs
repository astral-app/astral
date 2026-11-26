use astral_core::env_report::{
    AndroidFacts, AppleFacts, HttpsCheck, android_install_line, android_markers, apple_markers, crash_summary, https_verdict, is_beta_build,
};
use serde_json::json;

#[test]
fn marks_apple_installs() {
    let facts: AppleFacts = serde_json::from_value(json!({
        "playCover": true, "iOSAppOnMac": true, "jitEnabled": true, "refreshRate": 120,
        "duplicateCopy": "/a/b/Unbound.dylib", "locale": { "calendar": "buddhist", "rightToLeft": true }
    }))
    .unwrap();
    assert_eq!(
        apple_markers(&facts),
        vec![
            "second Unbound copy skipped (Unbound.dylib)",
            "PlayCover",
            "JIT enabled",
            "120 Hz",
            "buddhist calendar",
            "right-to-left language"
        ]
    );
}

#[test]
fn marks_android_installs() {
    let facts: AndroidFacts = serde_json::from_value(json!({
        "selinuxEnforcing": false, "verifiedBootState": "orange", "buildTags": "test-keys", "pageSize": 16384,
        "webView": "com.google.android.webview 110.0", "backgroundRestricted": true, "batteryOptimized": true,
        "skin": "One UI", "abiBridge": "libhoudini"
    }))
    .unwrap();
    assert_eq!(
        android_markers(&facts),
        vec![
            "SELinux permissive",
            "boot orange",
            "test-keys",
            "16 KB pages",
            "WebView com.google.android.webview 110.0",
            "One UI",
            "bg restricted",
            "battery optimized",
            "ABI translation (libhoudini)"
        ]
    );
}

#[test]
fn reports_https_and_install_and_crashes() {
    let public = HttpsCheck {
        completed: true,
        issuers: vec!["WR1".into(), "GTS Root R1".into()],
        failure: String::new(),
    };
    assert_eq!(https_verdict(&public), json!({"intercepted": false, "issuer": "GTS Root R1"}));
    let proxy = HttpsCheck {
        completed: true,
        issuers: vec!["Charles Proxy".into()],
        failure: String::new(),
    };
    assert_eq!(https_verdict(&proxy), json!({"intercepted": true, "issuer": "Charles Proxy"}));
    assert_eq!(https_verdict(&HttpsCheck::default()), serde_json::Value::Null);
    assert_eq!(
        android_install_line(&json!({"signer": "abcdef0123456789ff", "patch": "LSPatch", "missingSplits": ["config.arm64"], "splits": 3})),
        "APK cert abcdef0123456789 · re-signed · LSPatch · missing config.arm64"
    );
    let reports = json!([{"title": "Crash: x", "kind": "crash", "time": 200_000.0}, {"kind": "hang", "time": 0.0}]);
    assert_eq!(
        crash_summary(&reports, 200_000.0 + 3600.0),
        json!({"reports": 2, "crashes_24h": 1, "last_report": "Crash: x"})
    );
    assert!(is_beta_build("23A5276f"));
    assert!(!is_beta_build("23A341"));
}
