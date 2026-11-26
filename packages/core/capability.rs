macro_rules! capabilities {
    ($($variant:ident = $value:literal, $description:literal, $icon:literal;)+) => {
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

            pub const fn description(self) -> &'static str {
                match self {
                    $(Capability::$variant => $description),+
                }
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
    DiscordGuilds = "discord.guilds", "Reads servers and their roles", "res://ic_mana_server";
    DiscordMembers = "discord.members", "Reads server members", "res://ic_mana_group";
    DiscordPermissions = "discord.permissions", "Reads member permissions", "res://ic_mana_shield";
    DiscordMessages = "discord.messages", "Reads messages in the cache", "res://ic_mana_chat";
    DiscordMessagesContent = "discord.messages.content", "Reads cached message text", "res://ic_mana_chat";
    DiscordMessagesSend = "discord.messages.send", "Transforms message text before sending", "res://ic_mana_chat";
    MessagesDisplay = "messages.display", "Changes how message text is shown on this device", "res://ic_mana_chat";
    EventsMessages = "events.messages", "Receives new message metadata while Discord is open", "res://ic_mana_chat";
    EventsMessagesFilter = "events.messages.filter", "Reads and filters incoming messages before Discord handles them", "res://ic_mana_chat";
    EventsLifecycle = "events.lifecycle", "Receives app foreground and background events", "res://ic_mana_calendar";
    UiClipboard = "ui.clipboard", "Copies text to the system clipboard", "res://ic_mana_copy";
    NetDiscord = "net.discord", "Reads and changes Discord's own requests, without your token", "res://ic_mana_shield";
    EventsGateway = "events.gateway", "Reads and changes Discord's live events", "res://ic_mana_bell";
    MessagesLocal = "messages.local", "Shows local events and messages only you see", "res://ic_mana_chat";
    Background = "background", "Keeps Discord running in the background", "res://ic_mana_moon";
    Diagnostics = "diagnostics", "Reads astral's versions, device details and plugin states", "res://ic_mana_bug";
    Commands = "commands", "Adds slash commands", "res://ic_mana_slash";
    UiText = "ui.text", "Restyles the text of messages", "res://ic_mana_text";
    UiImages = "ui.images", "Changes which images Discord shows", "res://ic_mana_image";
    UiPress = "ui.press", "Presses Discord's own buttons", "res://ic_mana_touch";
    UiNavigate = "ui.navigate", "Opens channels and scrolls the chat", "res://ic_mana_link";
    UiComposer = "ui.composer", "Reads and styles draft text in the message composer", "res://ic_mana_chat";
    UiMessageBackground = "ui.message_background", "Styles message backgrounds", "res://ic_mana_chat";
    UiDialogs = "ui.dialogs", "Shows dialogs and pickers", "res://ic_mana_chat";
    UiPatch = "ui.patch", "Restyles parts of Discord's screens", "res://ic_mana_paint_palette";
    UiInsert = "ui.insert", "Adds buttons and text to Discord's screens", "res://ic_mana_paint_palette";
    UiCodeBlocks = "ui.code_blocks", "Restyles code blocks in messages", "res://ic_mana_paint_palette";
    FilesRead = "files.read", "Reads files you pick", "res://ic_mana_image";
    MediaSave = "media.save", "Changes which file Discord saves to your photos", "res://ic_mana_download";
    MediaPicker = "media.picker", "Reads the size and length of photos in the media picker", "res://ic_mana_image";
    MediaOffer = "media.offer", "Answers Discord's next photo picker with an image", "res://ic_mana_image";
    UiHaptics = "ui.haptics", "Triggers haptic feedback", "res://ic_mana_bell";
    UiToasts = "ui.toasts", "Shows brief notifications", "res://ic_mana_bell";
    UiNotifications = "ui.notifications", "Posts system notifications", "res://ic_mana_bell";
    Audio = "audio", "Plays sounds and reads text aloud", "res://ic_mana_bell";
    UiMessageActions = "ui.message_actions", "Adds actions to the message menu", "res://ic_mana_chat";
    UiWebAuth = "ui.web_auth", "Opens a secure sign-in window", "res://ic_mana_link";
    DiscordUsers = "discord.users", "Reads user profiles", "res://ic_mana_user";
    DiscordRelationships = "discord.relationships", "Reads your friends, blocks and their nicknames", "res://ic_mana_friends";
    Network = "network", "Makes HTTPS requests", "res://ic_mana_link";
    Storage = "storage", "Keeps its own data on this device", "res://ic_mana_download";
}
