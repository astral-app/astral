use astral_core::app::links::install_target;

#[test]
fn reads_install_links() {
    let target = |kind: &str, identifier: &str| Some((kind.to_owned(), identifier.to_owned()));
    assert_eq!(
        install_target("astral://install/plugin/astral.reviewdb"),
        target("plugin", "astral.reviewdb")
    );
    assert_eq!(
        install_target("astral://install/theme/midnight/?ref=x"),
        target("theme", "midnight")
    );
    assert_eq!(install_target("discord://app/astral/install/plugin/astral.translate"), None);
    assert_eq!(install_target("astral://install/font/x"), None);
    assert_eq!(install_target("astral://install/plugin/../etc"), None);
    assert_eq!(install_target("astral://-/channels/@me"), None);
    assert_eq!(install_target("https://astral/install/plugin/x"), None);
}
