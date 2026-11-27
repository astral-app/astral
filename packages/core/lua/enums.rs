use core::ffi::c_int;

use crate::lua::api::{self as lua, LuaState};

macro_rules! lua_enums {
    ($($enum_name:ident { $($member:ident = $value:literal),+ $(,)? })+) => {
        $(
            #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
            #[repr(u8)]
            pub enum $enum_name {
                #[default]
                $($member),+
            }

            impl $enum_name {
                pub const fn value(self) -> &'static str {
                    match self {
                        $(Self::$member => $value),+
                    }
                }

                pub fn parse(value: &str) -> Option<Self> {
                    match value {
                        $($value => Some(Self::$member),)+
                        _ => None,
                    }
                }
            }
        )+

        pub const ALL: &[(&str, &[(&str, &str)])] = &[
            $((stringify!($enum_name), &[$((stringify!($member), $value)),+])),+
        ];
    };
}

macro_rules! lua_number_enums {
    ($($enum_name:ident { $($member:ident = $value:literal),+ $(,)? })+) => {
        $(
            #[derive(Clone, Copy, Debug, PartialEq, Eq)]
            pub enum $enum_name {
                $($member),+
            }

            impl $enum_name {
                pub const fn value(self) -> i64 {
                    match self {
                        $(Self::$member => $value),+
                    }
                }

                pub const fn parse(value: i64) -> Option<Self> {
                    match value {
                        $($value => Some(Self::$member),)+
                        _ => None,
                    }
                }
            }
        )+

        pub const ALL_NUMBERS: &[(&str, &[(&str, i64)])] = &[
            $((stringify!($enum_name), &[$((stringify!($member), $value)),+])),+
        ];
    };
}

lua_number_enums! {
    TextStyle {
        Bold = 1,
        Italic = 2,
        Underline = 4,
    }
    MessageType {
        Default = 0,
        RecipientAdd = 1,
        RecipientRemove = 2,
        Call = 3,
        ChannelNameChange = 4,
        ChannelIconChange = 5,
        ChannelPinnedMessage = 6,
        UserJoin = 7,
        GuildBoost = 8,
        ThreadCreated = 18,
        Reply = 19,
        ChatInputCommand = 20,
        ThreadStarterMessage = 21,
        ContextMenuCommand = 23,
        AutoModerationAction = 24,
        PollResult = 46,
    }
}

lua_enums! {
    Target {
        MessageAuthor = "message.author",
        ChannelName = "channel.name",
        MemberName = "member.name",
        ComposerPlaceholder = "composer.placeholder",
        ProfileName = "profile.name",
    }
    NodeKind {
        Author = "author",
        Reply = "reply",
        Channel = "channel",
        Member = "member",
        Composer = "composer",
        Profile = "profile",
    }
    Shape {
        Pill = "pill",
        Op = "op",
        Text = "text",
    }
    Event {
        AppForeground = "app.foreground",
        AppBackground = "app.background",
        MessageCreate = "message.create",
        MessageFilter = "message.filter",
        Gateway = "gateway",
    }
    SheetItemType {
        Heading = "heading",
        Text = "text",
        Code = "code",
        Image = "image",
        Row = "row",
        Column = "column",
        Buttons = "buttons",
        Input = "input",
        Divider = "divider",
        Spacer = "spacer",
        Icon = "icon",
    }
    SheetAlign {
        Start = "start",
        Center = "center",
        End = "end",
        Top = "top",
    }
    SheetColor {
        Primary = "primary",
        Muted = "muted",
        Link = "link",
        Positive = "positive",
        Danger = "danger",
    }
    Direction {
        Row = "row",
        Column = "column",
    }
    ButtonStyle {
        Primary = "primary",
        Secondary = "secondary",
        Danger = "danger",
    }
    ScrollAlign {
        Start = "start",
        End = "end",
        Visible = "visible",
    }
    PatchNodeType {
        View = "view",
        Text = "text",
        Image = "image",
        Stack = "stack",
    }
    InsertNodeType {
        View = "view",
        Text = "text",
        Image = "image",
        Player = "player",
        Lottie = "lottie",
    }
    InsertPosition {
        Start = "start",
        End = "end",
    }
    Justify {
        Start = "start",
        Center = "center",
        End = "end",
    }
    Align {
        Start = "start",
        Center = "center",
        End = "end",
        Stretch = "stretch",
    }
    ReplyPlacement {
        Above = "above",
        Bubble = "bubble",
        Inside = "inside",
    }
    NamePlacement {
        Above = "above",
        Inside = "inside",
    }
    TimePlacement {
        Header = "header",
        Inline = "inline",
        Hidden = "hidden",
    }
    TextKind {
        Avatar = "avatar",
        Header = "header",
        Voter = "voter",
        Typing = "typing",
        Mention = "mention",
        Message = "message",
    }
    ImageItemType {
        Rect = "rect",
        Circle = "circle",
        Text = "text",
    }
    FontWeight {
        Regular = "regular",
        Semibold = "semibold",
        Bold = "bold",
    }
    TextAlign {
        Left = "left",
        Center = "center",
        Right = "right",
    }
    OptionType {
        String = "string",
        Integer = "integer",
        Number = "number",
        Boolean = "boolean",
        User = "user",
        Channel = "channel",
        Role = "role",
    }
    MediaKind {
        Video = "video",
        Image = "image",
    }
    PickedMediaType {
        Image = "image",
        Video = "video",
        Audio = "audio",
        Unknown = "unknown",
    }
    Relationship {
        Friend = "friend",
        Blocked = "blocked",
        Incoming = "incoming",
        Outgoing = "outgoing",
    }
}

impl Relationship {
    const DISCORD_FRIEND: i64 = 1;
    const DISCORD_BLOCKED: i64 = 2;
    const DISCORD_INCOMING: i64 = 3;
    const DISCORD_OUTGOING: i64 = 4;

    pub const fn from_discord(kind: i64) -> Option<Self> {
        match kind {
            Self::DISCORD_FRIEND => Some(Self::Friend),
            Self::DISCORD_BLOCKED => Some(Self::Blocked),
            Self::DISCORD_INCOMING => Some(Self::Incoming),
            Self::DISCORD_OUTGOING => Some(Self::Outgoing),
            _ => None,
        }
    }
}

impl OptionType {
    pub const fn discord_type(self) -> i64 {
        match self {
            Self::String => 3,
            Self::Integer => 4,
            Self::Boolean => 5,
            Self::User => 6,
            Self::Channel => 7,
            Self::Role => 8,
            Self::Number => 10,
        }
    }
}

impl TextKind {
    pub const fn from_wire(kind: i32) -> Self {
        match kind {
            crate::wire::abi::NAME_HEADER => Self::Header,
            crate::wire::abi::NAME_VOTER => Self::Voter,
            crate::wire::abi::NAME_TYPING => Self::Typing,
            _ => Self::Avatar,
        }
    }
}

pub unsafe fn push_all(state: *mut LuaState) {
    let count = c_int::try_from(ALL.len() + ALL_NUMBERS.len()).unwrap_or(0);
    unsafe { lua::lua_createtable(state, 0, count) };
    for (name, members) in ALL {
        let size = c_int::try_from(members.len()).unwrap_or(0);
        unsafe { lua::lua_createtable(state, 0, size) };
        for (member, value) in *members {
            unsafe {
                lua::push_bytes(state, value.as_bytes());
                lua::set_field(state, -2, member);
            }
        }
        unsafe { lua::set_field(state, -2, name) };
    }
    for (name, members) in ALL_NUMBERS {
        let size = c_int::try_from(members.len()).unwrap_or(0);
        unsafe { lua::lua_createtable(state, 0, size) };
        for (member, value) in *members {
            unsafe {
                lua::lua_pushinteger(state, *value);
                lua::set_field(state, -2, member);
            }
        }
        unsafe { lua::set_field(state, -2, name) };
    }
}
