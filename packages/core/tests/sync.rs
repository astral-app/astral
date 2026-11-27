use astral_core::app::sync::{should_apply, stamp_of, stamped};

#[test]
fn stamps_round_trip_and_decide_what_to_apply() {
    let backup = "format = 1\n\n[settings]\ntheme = \"astral\"\n";
    let file = stamped(backup, "phone", 1_700_000_000_000);
    let parsed: toml::Table = file.parse().expect("still valid TOML");
    assert_eq!(parsed["format"].as_integer(), Some(1));
    let stamp = stamp_of(&file).expect("stamp");
    assert_eq!((stamp.device.as_str(), stamp.updated_ms), ("phone", 1_700_000_000_000));
    assert!(should_apply(&stamp, "tablet", 0));
    assert!(!should_apply(&stamp, "phone", 0));
    assert!(!should_apply(&stamp, "tablet", 1_700_000_000_000));
    assert!(stamp_of(backup).is_none());
}
