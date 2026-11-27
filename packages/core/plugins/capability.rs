macro_rules! capabilities {
    ($($variant:ident = $value:literal, $description_key:literal, $icon:literal;)+) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum Capability {
            $($variant),+
        }

        impl Capability {
            pub const ALL: &[Capability] = &[$(Capability::$variant),+];

            pub const fn value(self) -> &'static str {
                match self {
                    $(Capability::$variant => $value),+
                }
            }

            pub const fn description_key(self) -> &'static str {
                match self {
                    $(Capability::$variant => $description_key),+
                }
            }

            pub fn description(self) -> &'static str {
                crate::support::translations::tr(self.description_key())
            }

            pub const fn icon(self) -> &'static str {
                match self {
                    $(Capability::$variant => $icon),+
                }
            }

            pub fn parse(value: &str) -> Option<Capability> {
                match value {
                    $($value => Some(Capability::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

capabilities! {
    DiscordGuilds = "discord.guilds", "ASTRAL_PERMISSION_DISCORD_GUILDS", "res://ic_mana_server";
    DiscordMembers = "discord.members", "ASTRAL_PERMISSION_DISCORD_MEMBERS", "res://ic_mana_group";
    DiscordPermissions = "discord.permissions", "ASTRAL_PERMISSION_DISCORD_PERMISSIONS", "res://ic_mana_shield";
    DiscordMessages = "discord.messages", "ASTRAL_PERMISSION_DISCORD_MESSAGES", "res://ic_mana_chat";
    DiscordMessagesContent = "discord.messages.content", "ASTRAL_PERMISSION_DISCORD_MESSAGES_CONTENT", "res://ic_mana_chat";
    DiscordApi = "discord.api", "ASTRAL_PERMISSION_DISCORD_API", "res://ic_mana_shield";
    DiscordMessagesSend = "discord.messages.send", "ASTRAL_PERMISSION_DISCORD_MESSAGES_SEND", "res://ic_mana_chat";
    MessagesDisplay = "messages.display", "ASTRAL_PERMISSION_MESSAGES_DISPLAY", "res://ic_mana_chat";
    EventsMessages = "events.messages", "ASTRAL_PERMISSION_EVENTS_MESSAGES", "res://ic_mana_chat";
    EventsMessagesFilter = "events.messages.filter", "ASTRAL_PERMISSION_EVENTS_MESSAGES_FILTER", "res://ic_mana_chat";
    EventsLifecycle = "events.lifecycle", "ASTRAL_PERMISSION_EVENTS_LIFECYCLE", "res://ic_mana_calendar";
    UiClipboard = "ui.clipboard", "ASTRAL_PERMISSION_UI_CLIPBOARD", "res://ic_mana_copy";
    NetDiscord = "net.discord", "ASTRAL_PERMISSION_NET_DISCORD", "res://ic_mana_shield";
    EventsGateway = "events.gateway", "ASTRAL_PERMISSION_EVENTS_GATEWAY", "res://ic_mana_bell";
    MessagesLocal = "messages.local", "ASTRAL_PERMISSION_MESSAGES_LOCAL", "res://ic_mana_chat";
    Background = "background", "ASTRAL_PERMISSION_BACKGROUND", "res://ic_mana_moon";
    Diagnostics = "diagnostics", "ASTRAL_PERMISSION_DIAGNOSTICS", "res://ic_mana_bug";
    Commands = "commands", "ASTRAL_PERMISSION_COMMANDS", "res://ic_mana_slash";
    UiText = "ui.text", "ASTRAL_PERMISSION_UI_TEXT", "res://ic_mana_text";
    UiImages = "ui.images", "ASTRAL_PERMISSION_UI_IMAGES", "res://ic_mana_image";
    UiPress = "ui.press", "ASTRAL_PERMISSION_UI_PRESS", "res://ic_mana_touch";
    UiNavigate = "ui.navigate", "ASTRAL_PERMISSION_UI_NAVIGATE", "res://ic_mana_link";
    UiComposer = "ui.composer", "ASTRAL_PERMISSION_UI_COMPOSER", "res://ic_mana_chat";
    UiMessageBackground = "ui.message_background", "ASTRAL_PERMISSION_UI_MESSAGE_BACKGROUND", "res://ic_mana_chat";
    UiDialogs = "ui.dialogs", "ASTRAL_PERMISSION_UI_DIALOGS", "res://ic_mana_chat";
    UiPatch = "ui.patch", "ASTRAL_PERMISSION_UI_PATCH", "res://ic_mana_paint_palette";
    UiInsert = "ui.insert", "ASTRAL_PERMISSION_UI_INSERT", "res://ic_mana_paint_palette";
    FilesRead = "files.read", "ASTRAL_PERMISSION_FILES_READ", "res://ic_mana_image";
    MediaSave = "media.save", "ASTRAL_PERMISSION_MEDIA_SAVE", "res://ic_mana_download";
    MediaPicker = "media.picker", "ASTRAL_PERMISSION_MEDIA_PICKER", "res://ic_mana_image";
    MediaOffer = "media.offer", "ASTRAL_PERMISSION_MEDIA_OFFER", "res://ic_mana_image";
    UiHaptics = "ui.haptics", "ASTRAL_PERMISSION_UI_HAPTICS", "res://ic_mana_bell";
    UiToasts = "ui.toasts", "ASTRAL_PERMISSION_UI_TOASTS", "res://ic_mana_bell";
    UiNotifications = "ui.notifications", "ASTRAL_PERMISSION_UI_NOTIFICATIONS", "res://ic_mana_bell";
    Audio = "audio", "ASTRAL_PERMISSION_AUDIO", "res://ic_mana_bell";
    UiMessageActions = "ui.message_actions", "ASTRAL_PERMISSION_UI_MESSAGE_ACTIONS", "res://ic_mana_chat";
    UiWebAuth = "ui.web_auth", "ASTRAL_PERMISSION_UI_WEB_AUTH", "res://ic_mana_link";
    DiscordUsers = "discord.users", "ASTRAL_PERMISSION_DISCORD_USERS", "res://ic_mana_user";
    DiscordRelationships = "discord.relationships", "ASTRAL_PERMISSION_DISCORD_RELATIONSHIPS", "res://ic_mana_friends";
    Network = "network", "ASTRAL_PERMISSION_NETWORK", "res://ic_mana_link";
    Storage = "storage", "ASTRAL_PERMISSION_STORAGE", "res://ic_mana_download";
}
