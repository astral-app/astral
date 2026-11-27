use crate::settings::navigator;

const SLIDER_PREFIX: &str = "astral.slider.";
const INSERTED_PREFIX: &str = "astral.tap.";
const MESSAGE_ACTION_PREFIX: &str = "astral.action.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pressable {
    Ignored,
    Slider,
    Navigator,
    Inserted,
    MessageAction,
}

pub fn pressable(identifier: &str) -> Pressable {
    if identifier.starts_with(SLIDER_PREFIX) {
        Pressable::Slider
    } else if identifier.starts_with(INSERTED_PREFIX) {
        Pressable::Inserted
    } else if identifier.starts_with(MESSAGE_ACTION_PREFIX) {
        Pressable::MessageAction
    } else if navigator::handles(identifier) {
        Pressable::Navigator
    } else {
        Pressable::Ignored
    }
}

pub fn press(identifier: &str) -> bool {
    match pressable(identifier) {
        Pressable::Navigator => {
            navigator::tap(identifier);
            true
        }
        Pressable::Inserted => crate::lua::host::nodes::press_inserted_node(identifier),
        Pressable::MessageAction => crate::lua::host::nodes::press_message_action(identifier),
        Pressable::Slider => true,
        Pressable::Ignored => false,
    }
}
