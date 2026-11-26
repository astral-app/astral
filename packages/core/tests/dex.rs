use astral_core::support::dex::anchored_classes;

#[test]
fn finds_nothing_in_a_missing_apk() {
    let found = anchored_classes("/nonexistent.apk", &["Request{method="]);
    assert_eq!(found["Request{method="], serde_json::json!([]));
}

#[test]
#[ignore]
fn finds_anchored_classes_in_a_pulled_apk() {
    let apk = std::env::var("ASTRAL_APK").expect("ASTRAL_APK");
    let anchors = [
        "Request{method=",
        "network interceptor ",
        "Null interceptor: ",
        "code < 0: ",
        "Cannot buffer entire body for content length: ",
        "h2_prior_knowledge",
        "byteCount < 0: ",
    ];
    let started = std::time::Instant::now();
    let found = anchored_classes(&apk, &anchors);
    println!("{} in {:?}", serde_json::to_string_pretty(&found).unwrap(), started.elapsed());
}
