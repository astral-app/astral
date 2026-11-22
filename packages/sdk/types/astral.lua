---@meta astral

---@alias astral.Argb integer
---@alias astral.ColorLike astral.Color|astral.Argb|string
---@alias astral.Snowflake string
---@alias astral.SettingValue boolean|string

---@class astral.Role
---@field id astral.Snowflake
---@field name string
---@field position integer
---@field colors? { primary_color?: integer, secondary_color?: integer, tertiary_color?: integer }

---@class astral.Guild
---@field id astral.Snowflake
---@field name string
---@field owner_id astral.Snowflake
---@field roles table<astral.Snowflake, astral.Role>

---@class astral.Member
---@field roles astral.Snowflake[]
---@field nick? string

---@class astral.User
---@field id astral.Snowflake
---@field username string
---@field global_name? string
---@field bot? boolean
---@field avatar? string

---@class astral.MessageReference
---@field channel_id astral.Snowflake
---@field message_id astral.Snowflake
---@field guild_id? astral.Snowflake

---@class astral.Message
---@field id astral.Snowflake
---@field channel_id astral.Snowflake
---@field type astral.MessageType
---@field author astral.User
---@field webhook_id? astral.Snowflake
---@field reference? astral.MessageReference

---@class astral.Node
---@field kind astral.NodeKind
---@field text string
---@field user_id? astral.Snowflake
---@field channel_id? astral.Snowflake
---@field guild_id? astral.Snowflake
---@field message_id? astral.Snowflake
local Node = {}
---@return astral.Message?
function Node:message() end
---@return astral.Guild?
function Node:guild() end
---@return astral.Member?
function Node:member() end
---@return astral.User?
function Node:user() end
---@return integer?
function Node:permissions() end
---@return boolean
function Node:is_bot() end

---@enum astral.Target
local Target = {
	MessageAuthor = "message.author",
	ChannelName = "channel.name",
	MemberName = "member.name",
	ComposerPlaceholder = "composer.placeholder",
}

---@enum astral.NodeKind
local NodeKind = {
	Author = "author",
	Reply = "reply",
	Channel = "channel",
	Member = "member",
	Composer = "composer",
}

---@enum astral.Shape
local Shape = { Pill = "pill", Op = "op" }

---@enum astral.MessageType
local MessageType = {
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

---@enum astral.Permission
local Permission = {
	KICK_MEMBERS = 2,
	BAN_MEMBERS = 4,
	ADMINISTRATOR = 8,
	MANAGE_CHANNELS = 16,
	MANAGE_GUILD = 32,
	MANAGE_MESSAGES = 8192,
	MUTE_MEMBERS = 4194304,
	DEAFEN_MEMBERS = 8388608,
	MOVE_MEMBERS = 16777216,
	MANAGE_ROLES = 268435456,
	MANAGE_WEBHOOKS = 536870912,
	MANAGE_THREADS = 17179869184,
	MODERATE_MEMBERS = 1099511627776,
}

---@class astral.Color
---@field value astral.Argb
---@field Black astral.Color
---@field White astral.Color
---@field Blurple astral.Color
---@overload fun(argb: astral.Argb): astral.Color
local Color = {}
---@param argb astral.Argb
---@return astral.Color
function Color.new(argb) end
---@param argb astral.Argb
---@return astral.Color
function Color.argb(argb) end
---@param red integer
---@param green integer
---@param blue integer
---@param alpha? integer
---@return astral.Color
function Color.rgb(red, green, blue, alpha) end
---@param text string
---@return astral.Color
function Color.hex(text) end
---@param value astral.ColorLike
---@return astral.Color
function Color.from(value) end
---@param backgrounds astral.ColorLike[]
---@return astral.Color
function Color.contrasting(backgrounds) end
---@param value any
---@return boolean
function Color.is(value) end
---@return integer red, integer green, integer blue, integer alpha
function Color:channels() end
---@return astral.Color
function Color:opaque() end
---@param alpha integer
---@return astral.Color
function Color:with_alpha(alpha) end
---@return boolean
function Color:is_transparent() end
---@return number
function Color:luminance() end

---@class astral.PillOptions
---@field color? astral.ColorLike
---@field colors? astral.ColorLike[]
---@field text_color? astral.ColorLike
---@field shape? astral.Shape

---@class astral.BuiltPill
---@field text string
---@field color astral.Argb
---@field text_color astral.Argb
---@field colors? astral.Argb[]
---@field shape astral.Shape

---@class astral.Pill
---@field text string
---@field shape astral.Shape
---@field stops astral.Color[]
---@field text_color? astral.Color
---@overload fun(text: string, options?: astral.PillOptions): astral.Pill
local Pill = {}
---@param text string
---@param options? astral.PillOptions
---@return astral.Pill
function Pill.new(text, options) end
---@param value any
---@return boolean
function Pill.is(value) end
---@param color astral.ColorLike
---@return astral.Pill
function Pill:color(color) end
---@param colors astral.ColorLike[]
---@return astral.Pill
function Pill:gradient(colors) end
---@param color astral.ColorLike
---@return astral.Pill
function Pill:text_colored(color) end
---@param shape astral.Shape
---@return astral.Pill
function Pill:shaped(shape) end
---@return astral.BuiltPill
function Pill:build() end

---@class astral.Class
---@field new fun(...): any
---@field is fun(value: any): boolean
---@field super? astral.Class

---@class astral.SettingOptions
---@field subtitle? string
---@field section? string

---@class astral.ChoiceOption
---@field value string
---@field title string
---@field subtitle? string

---@class astral.Section
---@field title string
local Section = {}
---@param key string
---@param title string
---@param default boolean
---@param subtitle? string
---@return boolean
function Section:switch(key, title, default, subtitle) end
---@param key string
---@param title string
---@param choices astral.ChoiceOption[]
---@param default string
---@param subtitle? string
---@return string
function Section:choice(key, title, choices, default, subtitle) end

---@alias astral.Decoration astral.Pill|astral.PillOptions|(astral.Pill|astral.PillOptions)[]|nil

---@class astral
---@field id string
---@field api string
astral = {}
astral.Target = Target
astral.NodeKind = NodeKind
astral.Shape = Shape
astral.MessageType = MessageType
astral.Permission = Permission
astral.Color = Color
astral.Pill = Pill

---@param ... any
function astral.log(...) end

---@generic T: table
---@param name string
---@param members T
---@return T
function astral.enum(name, members) end

---@param name string
---@param parent? astral.Class
---@return astral.Class|table
function astral.class(name, parent) end

---@param key string
---@param value astral.SettingValue
function astral.on_setting(key, value) end

astral.settings = {}
---@type table<string, astral.SettingValue>
astral.settings.values = {}
---@param key string
---@param title string
---@param default boolean
---@param options? astral.SettingOptions
---@return boolean
function astral.settings.switch(key, title, default, options) end
---@param key string
---@param title string
---@param choices astral.ChoiceOption[]
---@param default string
---@param options? astral.SettingOptions
---@return string
function astral.settings.choice(key, title, choices, default, options) end
---@param key string
---@return astral.SettingValue?
function astral.settings.get(key) end
---@param key string
---@param value astral.SettingValue
function astral.settings.set(key, value) end
---@param title string
---@return astral.Section
function astral.settings.section(title) end
---@param value string
---@param title string
---@param subtitle? string
---@return astral.ChoiceOption
function astral.settings.option(value, title, subtitle) end
---@param key string
---@param listener fun(value: astral.SettingValue, key: string)
function astral.settings.on_change(key, listener) end

astral.i18n = {}
---@type string
astral.i18n.locale = ""
---@param key string
---@return string
function astral.i18n.t(key) end

astral.ui = {}
---@param target astral.Target
---@param decorator fun(node: astral.Node): astral.Decoration
function astral.ui.decorate(target, decorator) end
---@param target astral.Target
---@param relabeler fun(node: astral.Node): string?
function astral.ui.relabel(target, relabeler) end

astral.discord = {}
---@param id astral.Snowflake
---@return astral.Guild?
function astral.discord.guild(id) end
---@param guild astral.Snowflake
---@param user astral.Snowflake
---@return astral.Member?
function astral.discord.member(guild, user) end
---@param channel astral.Snowflake
---@return astral.Snowflake?
function astral.discord.channel_guild(channel) end
---@param guild astral.Snowflake
---@param user astral.Snowflake
---@param channel? astral.Snowflake
---@return integer?
function astral.discord.permissions(guild, user, channel) end
---@param channel astral.Snowflake
---@param message astral.Snowflake
---@return astral.Message?
function astral.discord.message(channel, message) end
---@param id astral.Snowflake
---@return astral.User?
function astral.discord.user(id) end
---@param id astral.Snowflake
---@return string?
function astral.discord.nickname(id) end

astral.permissions = {}
---@param ... astral.Permission
---@return integer
function astral.permissions.combine(...) end
---@param granted integer?
---@param ... astral.Permission
---@return boolean
function astral.permissions.any(granted, ...) end
---@param granted integer?
---@param ... astral.Permission
---@return boolean
function astral.permissions.all(granted, ...) end

astral.list = {}
---@generic T, R
---@param items T[]
---@param transform fun(item: T, index: integer): R
---@return R[]
function astral.list.map(items, transform) end
---@generic T
---@param items T[]
---@param keep fun(item: T, index: integer): boolean
---@return T[]
function astral.list.filter(items, keep) end
---@generic T
---@param items T[]
---@param matches fun(item: T, index: integer): boolean
---@return T?, integer?
function astral.list.find(items, matches) end
---@generic T
---@param items T[]
---@param wanted T
---@return boolean
function astral.list.contains(items, wanted) end
---@generic T
---@param items T[]
---@param item T
---@return T[]
function astral.list.push_unique(items, item) end
