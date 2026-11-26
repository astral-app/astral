use astral_core::environment::*;

#[test]
fn names_install_sources() {
    let base = InstallFacts {
        simulator: false,
        receipt: RECEIPT_NONE,
        troll_store: false,
        troll_store_lite: true,
        live_container: false,
        jit: false,
        discord_signed: None,
    };
    assert_eq!(app_source(&base), "TrollStore Lite");
    let sideloaded = InstallFacts {
        receipt: RECEIPT_TEST_FLIGHT,
        troll_store_lite: false,
        discord_signed: Some(false),
        ..base
    };
    assert_eq!(app_source(&sideloaded), "Sideloaded (TestFlight build)");
    assert_eq!(receipt_kind(Some(b"sandboxReceipt"), true), RECEIPT_TEST_FLIGHT);
    assert_eq!(receipt_kind(Some(b"receipt"), false), RECEIPT_NONE);
}

#[test]
fn describes_signing() {
    let facts = SigningFacts {
        team_identifier: Some(b"ABCDE12345"),
        get_task_allow: false,
        has_profile: true,
        provisions_all_devices: true,
        lists_devices: false,
        profile_expiry: Some(b"2027-01-01"),
        team_name: Some(" Team `X` · Corp ".as_bytes()),
        sideloader: b"",
    };
    assert_eq!(
        String::from_utf8(signing_details(&facts)).unwrap(),
        "Enterprise (`ABCDE12345`) · Team X - Corp · exp 2027-01-01"
    );
    let discord = SigningFacts {
        team_identifier: Some(b"53Q6R32WPB"),
        ..facts
    };
    assert_eq!(String::from_utf8(signing_details(&discord)).unwrap(), "Distribution (`53Q6R32WPB`)");
}

#[test]
fn reads_jailbreak_markers() {
    let markers = vec![b".installed_dopamine".to_vec()];
    assert_eq!(jailbreak_name(b"/var/jb", &markers, &[], false), "Dopamine");
    assert_eq!(
        jailbreak_name(b"/private/var/.jbroot-1234", &markers, &[], false),
        "Dopamine (roothide)"
    );
    assert_eq!(jailbreak_label(b"Dopamine", b"2.4.1"), b"Dopamine 2.4.1");
    assert_eq!(jailbreak_label(b"Dopamine", b"2..1"), b"Dopamine");
    assert_eq!(bootstrap_root_of_image(b"/var/jb/usr/lib/libellekit.dylib").unwrap(), b"/var/jb");
    assert_eq!(
        bootstrap_root_of_image(b"/private/var/.jbroot-AB/usr/lib/x.dylib").unwrap(),
        b"/private/var/.jbroot-AB"
    );
    assert_eq!(
        hook_framework_for_image(b"/usr/lib/libSubstrate.dylib", true),
        Some(("ElleKit", "ellekit"))
    );
    let status = b"Package: ellekit\r\nVersion: 1.1.3\r\n\r\nPackage: other\nVersion: 9\n";
    assert_eq!(dpkg_package_version(status, b"ellekit").unwrap(), b"1.1.3");
    assert_eq!(dpkg_package_version(status, b"missing"), None);
}

#[test]
fn finds_the_default_gateway() {
    let mut message = vec![0u8; 92];
    message[12..16].copy_from_slice(&3i32.to_ne_bytes());
    let mut destination = vec![0u8; 16];
    destination[0] = 16;
    destination[1] = 2;
    let mut gateway = destination.clone();
    gateway[4..8].copy_from_slice(&[192, 168, 1, 1]);
    message.extend_from_slice(&destination);
    message.extend_from_slice(&gateway);
    let length = message.len() as u16;
    message[0..2].copy_from_slice(&length.to_ne_bytes());
    assert_eq!(default_ipv4_gateway(&message).as_deref(), Some("192.168.1.1"));
    assert_eq!(default_ipv4_gateway(&message[..50]), None);
}
