use astral_core::app::pressable::{Pressable, pressable};

#[test]
fn sorts_identifiers_by_what_a_press_does() {
    let cases = [
        ("astral.slider.4.scale", Pressable::Slider),
        ("astral.tap.plugin:open:key", Pressable::Inserted),
        ("astral.action.plugin.copy", Pressable::MessageAction),
        ("settings.row.astral", Pressable::Navigator),
        ("settings.row.astral.plugins", Pressable::Navigator),
        ("astral.back", Pressable::Navigator),
        ("astral.row.theme", Pressable::Navigator),
        ("astral.switch.compact", Pressable::Navigator),
        ("settings.row.appearance", Pressable::Ignored),
        ("", Pressable::Ignored),
    ];
    for (identifier, expected) in cases {
        assert_eq!(pressable(identifier), expected, "{identifier}");
    }
}
