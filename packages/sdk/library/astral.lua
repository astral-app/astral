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
---@field joined_at? string
---@field flags? integer

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
---@field content? string cached text; needs discord.messages.content
---@field flags integer Discord's message flags, such as 4 for suppressed embeds
---@field embeds integer how many embeds the message has
---@field reactions { emoji_id?: astral.Snowflake, emoji_name?: string, count: integer, burst_count: integer, me: boolean }[]

---@class astral.Node
---@field kind astral.NodeKind
---@field text string
---@field user_id? astral.Snowflake
---@field author_id? astral.Snowflake
---@field channel_id? astral.Snowflake
---@field guild_id? astral.Snowflake
---@field message_id? astral.Snowflake
---@field emoji? string
---@field burst? boolean
---@field count? integer
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
	ProfileName = "profile.name",
}

---@enum astral.NodeKind
local NodeKind = {
	Author = "author",
	Reply = "reply",
	Channel = "channel",
	Member = "member",
	Composer = "composer",
	Profile = "profile",
}

---@enum astral.Shape
local Shape = {
	Pill = "pill",
	Op = "op",
}

---@enum astral.Event
local Event = {
	AppForeground = "app.foreground",
	AppBackground = "app.background",
	MessageCreate = "message.create",
	MessageFilter = "message.filter",
	Gateway = "gateway",
}

---@enum astral.SheetItemType
local SheetItemType = {
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

---@enum astral.SheetAlign
local SheetAlign = {
	Start = "start",
	Center = "center",
	End = "end",
	Top = "top",
}

---@enum astral.SheetColor
local SheetColor = {
	Primary = "primary",
	Muted = "muted",
	Link = "link",
	Positive = "positive",
	Danger = "danger",
}

---@enum astral.Direction
local Direction = {
	Row = "row",
	Column = "column",
}

---@enum astral.ButtonStyle
local ButtonStyle = {
	Primary = "primary",
	Secondary = "secondary",
	Danger = "danger",
}

---@enum astral.ScrollAlign
local ScrollAlign = {
	Start = "start",
	End = "end",
	Visible = "visible",
}

---@enum astral.PatchNodeType
local PatchNodeType = {
	View = "view",
	Text = "text",
	Image = "image",
	Stack = "stack",
}

---@enum astral.InsertNodeType
local InsertNodeType = {
	View = "view",
	Text = "text",
	Image = "image",
	Player = "player",
	Lottie = "lottie",
}

---@enum astral.InsertPosition
local InsertPosition = {
	Start = "start",
	End = "end",
}

---@enum astral.Justify
local Justify = {
	Start = "start",
	Center = "center",
	End = "end",
}

---@enum astral.Align
local Align = {
	Start = "start",
	Center = "center",
	End = "end",
	Stretch = "stretch",
}

---@enum astral.ReplyPlacement
local ReplyPlacement = {
	Above = "above",
	Bubble = "bubble",
	Inside = "inside",
}

---@enum astral.NamePlacement
local NamePlacement = {
	Above = "above",
	Inside = "inside",
}

---@enum astral.TimePlacement
local TimePlacement = {
	Header = "header",
	Inline = "inline",
	Hidden = "hidden",
}

---@enum astral.TextKind
local TextKind = {
	Avatar = "avatar",
	Header = "header",
	Voter = "voter",
	Typing = "typing",
	Mention = "mention",
	Message = "message",
}

---@enum astral.ImageItemType
local ImageItemType = {
	Rect = "rect",
	Circle = "circle",
	Text = "text",
}

---@enum astral.FontWeight
local FontWeight = {
	Regular = "regular",
	Semibold = "semibold",
	Bold = "bold",
}

---@enum astral.TextAlign
local TextAlign = {
	Left = "left",
	Center = "center",
	Right = "right",
}

---@enum astral.OptionType
local OptionType = {
	String = "string",
	Integer = "integer",
	Number = "number",
	Boolean = "boolean",
	User = "user",
	Channel = "channel",
	Role = "role",
}

---@enum astral.MediaKind
local MediaKind = {
	Video = "video",
	Image = "image",
}

---@enum astral.PickedMediaType
local PickedMediaType = {
	Image = "image",
	Video = "video",
	Audio = "audio",
	Unknown = "unknown",
}

---@enum astral.Relationship
local Relationship = {
	Friend = "friend",
	Blocked = "blocked",
	Incoming = "incoming",
	Outgoing = "outgoing",
}

---@enum astral.TextStyle
local TextStyle = {
	Bold = 1,
	Italic = 2,
	Underline = 4,
}

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
	EMBED_LINKS = 16384,
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
---@field icon? string
---@field tint? astral.ColorLike

---@class astral.BuiltPill
---@field text string
---@field color astral.Argb
---@field text_color astral.Argb
---@field colors? astral.Argb[]
---@field shape astral.Shape
---@field icon? string
---@field tint? astral.Argb

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

---@class astral.NumberOptions
---@field min number
---@field max number
---@field step? number
---@field default? number
---@field subtitle? string
---@field section? string
---@field icon? string

---@class astral.SettingOptions
---@field subtitle? string
---@field section? string
---@field icon? string

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
---@param options? string|astral.SettingOptions
---@return boolean
function Section:switch(key, title, default, options) end
---@param key string
---@param title string
---@param callback fun()
---@param options? string|{ subtitle: string?, icon: string?, detail: string? }
function Section:button(key, title, callback, options) end
---@param key string
---@param title string
---@param default? string
---@param options? string|astral.SettingOptions
---@return string
function Section:color(key, title, default, options) end
function Section:text(key, title, default, options) end
---@param key string
---@param title string
---@param options astral.NumberOptions
---@return number
function Section:number(key, title, options) end
---@param key string
---@param title string
---@param choices astral.ChoiceOption[]
---@param default string
---@param options? string|astral.SettingOptions
---@return string
function Section:choice(key, title, choices, default, options) end

---@alias astral.Decoration astral.Pill|astral.PillOptions|(astral.Pill|astral.PillOptions)[]|nil

---@class astral
---@field id string
---@field api string
astral = {}
astral.Target = Target
astral.NodeKind = NodeKind
astral.Shape = Shape
astral.Event = Event
astral.SheetItemType = SheetItemType
astral.SheetAlign = SheetAlign
astral.SheetColor = SheetColor
astral.Direction = Direction
astral.ButtonStyle = ButtonStyle
astral.ScrollAlign = ScrollAlign
astral.PatchNodeType = PatchNodeType
astral.InsertNodeType = InsertNodeType
astral.InsertPosition = InsertPosition
astral.Justify = Justify
astral.Align = Align
astral.ReplyPlacement = ReplyPlacement
astral.NamePlacement = NamePlacement
astral.TimePlacement = TimePlacement
astral.TextKind = TextKind
astral.ImageItemType = ImageItemType
astral.FontWeight = FontWeight
astral.TextAlign = TextAlign
astral.OptionType = OptionType
astral.MediaKind = MediaKind
astral.PickedMediaType = PickedMediaType
astral.Relationship = Relationship
astral.TextStyle = TextStyle
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

---@type table<string, any>
astral.state = {}

---@param previous table<string, any>
function astral.on_reload(previous) end

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
---@param title string
---@param options astral.NumberOptions
---@return number
function astral.settings.number(key, title, options) end
function astral.settings.text(key, title, default, options) end
---@param key string
---@param title string
---@param default? string
---@param options? astral.SettingOptions
---@return string
function astral.settings.color(key, title, default, options) end
---Adds a tappable row to the plugin's settings page that calls `callback` when pressed.
---@param key string
---@param title string
---@param callback fun()
---@param options? { subtitle: string?, section: string?, icon: string?, detail: string? }
function astral.settings.button(key, title, callback, options) end
---@param key string
---@return astral.SettingValue?
function astral.settings.get(key, fallback) end
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

astral.audio = {}
---@param text string
---@param options? { language: string?, rate: number? }
---@return boolean
function astral.audio.speak(text, options) end
---@param file string
---@param options? { volume: number? }
---@return boolean
function astral.audio.play(file, options) end
function astral.audio.stop() end

astral.time = {}
function astral.time.now_ms() end
---@param delay_ms integer
---@param callback fun()
---@return integer timer
function astral.time.after(delay_ms, callback) end
---@param interval_ms integer
---@param callback fun()
---@return integer timer
function astral.time.every(interval_ms, callback) end
astral.base64 = {}
---Encodes bytes as standard base64.
---@param text string
---@return string
function astral.base64.encode(text) end
---Decodes standard or URL-safe base64, or returns nil for anything else.
---@param text string
---@return string?
function astral.base64.decode(text) end
astral.snowflake = {}
---Milliseconds since 1970 for Discord's epoch (2015-01-01).
astral.snowflake.EPOCH = 1420070400000
---When a Discord id was created, in Unix milliseconds, or nil for anything that isn't an id.
---@param id string
---@return integer?
function astral.snowflake.timestamp(id) end
---The smallest id created at a Unix time in milliseconds, for `before`/`after` queries.
---@param milliseconds integer
---@return string?
function astral.snowflake.from_time(milliseconds) end
---Whether a string is a Discord id.
---@param id any
---@return boolean
function astral.snowflake.is_valid(id) end
---Orders two ids by creation: -1, 0 or 1, or nil when either isn't an id.
---@param left string
---@param right string
---@return integer?
function astral.snowflake.compare(left, right) end
---Formats a time, or now, as "YYYY-MM-DD HH:MM UTC", or in the device's time zone without the suffix with `{ local_time = true }`.
---@param milliseconds? integer
---@param options? { local_time: boolean? }
---@return string
function astral.time.iso(milliseconds, options) end
---@param timer integer
function astral.time.cancel(timer) end

astral.ui = {}
function astral.ui.alert(title, message) end
---@class astral.ConfirmSpec
---@field title string
---@field message? string
---@field image? string https URL shown above the title
---@field confirm? string
---@field cancel? string
---@field destructive? boolean
---Asks a yes/no question in a native sheet and calls `callback(accepted)`.
---@param spec astral.ConfirmSpec
---@param callback fun(accepted: boolean)
---@return integer request
function astral.ui.confirm(spec, callback) end
---@class astral.SheetItem
---@field type astral.SheetItemType
---@field text? string
---@field url? string https image for `image`
---@field items? astral.SheetItem[] children of `row` and `column`
---@field buttons? { label: string, action: string, style: astral.ButtonStyle? }[]
---@field direction? astral.Direction how `buttons` are laid out
---@field align? astral.SheetAlign where text sits, or `astral.SheetAlign.Top` to top-align a row's children
---@field color? astral.SheetColor|astral.Argb|string text or icon colour
---@field name? string Discord icon name for `icon`
---@field size? number font, icon, image or spacer size
---@field bold? boolean
---@field round? boolean a circular `image`
---@field spacing? number gap between the children of `row` and `column`
---@field lines? integer at most this many lines of text
---@field action? string sent to the handler when the item is tapped or the input is submitted
---@field height? number fixed height of a scrolling `code` block, or its minimum with `fill`
---@field fill? boolean a `code` block grows into the sheet's free space and scrolls inside it
---@class astral.Sheet
---@field title string
---@field items astral.SheetItem[]
---@field expanded? boolean open at full height
---@class astral.SheetHandle
---@field id integer
---@field update fun(self: astral.SheetHandle, spec: astral.Sheet)
---@field close fun(self: astral.SheetHandle)
---Opens a Discord-styled bottom sheet. `handler(action, value)` gets taps and submitted input, then `"closed"` once.
---@param spec astral.Sheet
---@param handler fun(action: string, value: string)
---@return astral.SheetHandle
function astral.ui.sheet(spec, handler) end
function astral.ui.toast(title, message) end
function astral.ui.web_auth(url, callback_url, callback) end
function astral.ui.cancel_web_auth(request) end
function astral.ui.copy(value) end
---@return string?
function astral.ui.paste() end
---@param url string
---@return boolean
function astral.ui.open(url) end
---Opens a channel inside Discord, optionally at a message.
---@param channel_id string
---@param message_id? string
---@return boolean opened
function astral.ui.open_channel(channel_id, message_id) end
---@class astral.ScrollOptions
---@field animated? boolean defaults to true
---@field align? astral.ScrollAlign defaults to `astral.ScrollAlign.Visible`, the least scrolling that shows it
---Scrolls the open chat to a message that is loaded in it.
---@param message_id string
---@param options? astral.ScrollOptions
---@return boolean scrolled false when the message is not loaded
function astral.ui.scroll_to(message_id, options) end
---@class astral.TextSpan
---@field text? string
---@field color? astral.ColorLike
---@field weight? integer
---@field link? string
---@field font? string
---@field bg? integer background colour behind the span, as ARGB
---@field bg_radius? number
---@field scale? number
---@field bold? boolean
---@field strike? boolean
---@field underline? boolean
---Changes how message text is drawn. Return a new list of spans, or nothing to keep Discord's.
---@param transform fun(spans: astral.TextSpan[], context: { message_id: string, author_id?: string }): astral.TextSpan[]?
function astral.ui.text_transform(transform) end
---Rewrites the URL of every image Discord draws whose URL starts with `prefix`.
---@param prefix string
---@param rewrite fun(url: string): string?
function astral.ui.image_rewrite(prefix, rewrite) end
---Presses Discord's own control with this accessibility id, or the first starting with it when it ends in "*".
---@param identifier string
---@return boolean pressed
function astral.ui.press(identifier) end
---The text in the chat input right now.
---@return string
function astral.ui.composer_text() end
---Runs every decoration, patch, insert and text transform again on what is on screen.
function astral.ui.refresh() end
---Types text into the chat input at the cursor, or replaces all of it.
---@param text string
---@param options? { replace?: boolean }
---@return boolean inserted
function astral.ui.insert_text(text, options) end
---Focuses the chat input, or hides the keyboard when `focus` is false.
---@param focus? boolean defaults to true
---@return boolean
function astral.ui.focus_composer(focus) end
---@param title string
---@param body string
---@param options? { delay: number?, channel_id: string?, message_id: string? }
---@return boolean
function astral.ui.notify(title, body, options) end
function astral.ui.haptic(strength) end
function astral.ui.composer(formatter) end
---Closes the sheets and pages Discord shows over its main screen.
---@return boolean dismissed
function astral.ui.dismiss() end
---@param target astral.Target
---@param decorator fun(node: astral.Node): astral.Decoration
function astral.ui.decorate(target, decorator) end
---@param target astral.Target
---@param relabeler fun(node: astral.Node): string?
function astral.ui.relabel(target, relabeler) end
---@class astral.TextRunStyle
---@field color? astral.Color|integer|string the run's text colour
---@field image? string an https image shown beside the run (typing names and mentions)
---@field round? boolean round the image; true by default
---@field hide_at? boolean drop the @ of a mention
---@field after? boolean put a mention's image after its text
---Styles runs of Discord's text: message author names, member-list headers, poll voters, typing names, mentions and whole message text. `node.kind` says which; return a style, a colour, or nil.
---@param styler fun(node: astral.Node): astral.TextRunStyle|astral.Color|integer|string|nil
function astral.ui.style_text(styler) end
---@class astral.PatchSelector
---@field id string accessibilityId prefix, such as "message.content:"
---@field type? astral.PatchNodeType

---@class astral.PatchTarget
---@field id string
---@field key string the part of id after the prefix, such as a message ID
---@field type string
---@field message_id? string
---@field author_id? string
---@field channel_id? string the open channel
---@field parent_id? string the accessibility id of an image selector's parent
---@field node? integer the node's id; a new node means Discord drew it again
---@field checked? boolean whether a checkbox or switch is on, as Discord drew it
---@field label? string the node's accessibility label

---@class astral.NodePatch
---@field background? astral.Color|integer|string
---@field radius? number
---@field opacity? number
---@field padding? number|number[]
---@field margin? number|number[]
---@field border_color? astral.Color|integer|string
---@field border_width? number
---@field text_color? astral.Color|integer|string
---@field hidden? boolean
---@field text_scale? number 0.5 to 3, scales every text inside the node
---@field gradient? { angle?: number, colors: (astral.Color|integer|string)[] } a background gradient of up to 8 colours
---@field see_through? boolean children painted with this node's own background turn transparent, so a new background shows through

---@param selector string|astral.PatchSelector
---@param patcher fun(node: astral.PatchTarget): astral.NodePatch?
function astral.ui.patch(selector, patcher) end
---@class astral.InsertNode
---@field type? astral.InsertNodeType
---@field uri? string what a player plays: an https or content URL
---@field muted? boolean a player plays without sound
---@field fill_height? boolean take the full height of the parent
---@field cover? boolean an image fills its box, cropping instead of fitting
---@field fixed? boolean never shrink to make room for siblings
---@field text? string
---@field icon? string a Discord icon name, res:// URI, https URL or a png in the plugin folder
---@field color? astral.Color|integer|string text colour, or the tint of an image
---@field background? astral.Color|integer|string
---@field radius? number
---@field padding? number|number[]
---@field margin? number|number[]
---@field size? number font size
---@field width? number
---@field height? number
---@field row? boolean lay children out in a row
---@field justify? astral.Justify where children sit along the main axis
---@field align? astral.Align where children sit across it
---@field fill? boolean take the full width of the parent
---@field bold? boolean
---@field weight? integer font weight from 100 to 900; bold is 600
---@field action? string passed to on_press when the node is tapped
---@field position? astral.InsertPosition where the nodes go among the target's children
---@field children? astral.InsertNode[]

---@param selector string|astral.PatchSelector
---@param build fun(node: astral.PatchTarget): astral.InsertNode|astral.InsertNode[]|nil
---@param on_press? fun(node: astral.PatchTarget, action: string)
function astral.ui.insert(selector, build, on_press) end
---@class astral.MessageBackgroundStyle
---@field color astral.Color|integer|string
---@field radius? number
---@field bubble? boolean
---@field reply? astral.ReplyPlacement
---@field name? astral.NamePlacement where the author line sits on the first message of a group
---@field time? astral.TimePlacement where the time sits: in the author line, beside the text, or nowhere
---@param decorator fun(node: astral.Node): astral.MessageBackgroundStyle?
function astral.ui.message_background(decorator) end
---@class astral.CodeBlock
---@field message_id string
---@field channel_id? string
---@field index integer which code block of the message, from 0
---@field code string
---@class astral.CodeCardHeader
---@field label string
---@field icon? string image URI shown before the label
---@field iconTint? integer
---@field copy? string label of the copy button; the press handler receives the action "copy"
---@field background integer
---@field foreground integer
---@class astral.CodeCard
---@field background integer
---@field foreground integer
---@field lineNumbers? integer colour of the line numbers; leave out to hide them
---@field fontSize? number
---@field lineHeight? number
---@field lines {[1]: string, [2]: integer, [3]: integer}[][] runs of text, ARGB colour and `astral.TextStyle` flags combined with `|`
---@field header? astral.CodeCardHeader
---@param render fun(block: astral.CodeBlock): astral.CodeCard?
---@param press? fun(block: astral.CodeBlock, action: string)
function astral.ui.code_block(render, press) end
---@class astral.MessageActionOptions
---@field icon? string
---@field danger? boolean draws the row in Discord's red destructive style, like Delete
---@field when? fun(node: astral.Node): boolean shows the action only for messages it returns true for
---@param label string
---@param callback fun(node: astral.Node)
---@param options? astral.MessageActionOptions
function astral.ui.message_action(label, callback, options) end
---@class astral.Menu : astral.Node
---@field title string the menu's title, such as a server or folder name
---@field guild_id? astral.Snowflake the server the menu is for, when its title names exactly one cached server
---@class astral.MenuActionOptions
---@field icon? string
---@field danger? boolean
---@field when? fun(menu: astral.Menu): boolean shows the action only in menus it returns true for
---Adds a row to Discord's popover menus, such as the one a long-pressed server opens. Needs `ui.message_actions`.
---@param label string
---@param callback fun(menu: astral.Menu)
---@param options? astral.MenuActionOptions
function astral.ui.menu_action(label, callback, options) end
---Discord's current app background gradient, or the active theme's, as ARGB colours.
---@return { angle: number, colors: integer[] }?
function astral.ui.app_background() end

astral.files = {}
function astral.files.pick(extensions, callback) end
function astral.files.cancel(request) end

astral.events = {}
---@class astral.GatewayEvent
---@field type string the dispatch type, such as "MESSAGE_CREATE"
---@field seq? integer
---@field data any the event's payload
---Subscribes to an event. `astral.Event.Gateway` sees every Discord gateway dispatch, or only the types listed in `types`;
---return `false` to drop the event or a table to replace its data.
---@param event astral.Event
---@param callback fun(event: any): any
---@param types? string[] gateway event types to receive
---@return integer subscription
function astral.events.on(event, callback, types) end

astral.gateway = {}
---Delivers an event to Discord as if the gateway sent it. Only this device sees it.
---@param type string such as "MESSAGE_CREATE"
---@param data table
---@return boolean delivered
function astral.gateway.dispatch(type, data) end

astral.image = {}
---@class astral.ImageItem
---@field type astral.ImageItemType
---@field x number
---@field y number
---@field w? number rect width
---@field h? number rect height
---@field r? number circle radius
---@field radius? number rect corner radius
---@field color? integer ARGB
---@field border? integer ARGB
---@field border_width? number
---@field text? string
---@field size? number
---@field weight? astral.FontWeight
---@field mono? boolean
---@field width? number text width; longer text is cut with an ellipsis
---@field align? astral.TextAlign
---@class astral.ImageSpec
---@field width number points, up to 2048
---@field height number points, up to 4096
---@field scale? number 1 to 3, defaults to 2
---@field background? integer ARGB
---@field items astral.ImageItem[]
---Draws shapes and text into a PNG on this device.
---@param spec astral.ImageSpec
---@return string? png
function astral.image.render(spec) end

astral.background = {}
---Keeps Discord connected while it is in the background. Returns false where that isn't available.
---@param on boolean
---@return boolean
function astral.background.keep_alive(on) end

astral.diagnostics = {}
---Returns astral's sanitized environment: versions, platform, install mode, device, report count and every plugin's state.
---@return table
function astral.diagnostics.environment() end

astral.commands = {}
---@class astral.CommandOption
---@field name string
---@field description? string
---@field type? astral.OptionType
---@field required? boolean
---@class astral.Command
---@field name string lower-case, up to 32 characters
---@field description string
---@field options? astral.CommandOption[]
---@field execute fun(args: table<string, any>, context: { channel_id: string, guild_id?: string }): string|table|nil
---Adds a slash command to Discord's own command picker. Return text or a message to reply only to you,
---or `{ send = { content, file = { name, data, content_type } } }` to have Discord send it to the channel. Return nil to answer later with `astral.messages.send_local`.
---@param command astral.Command
function astral.commands.register(command) end

astral.json = {}
---@param text string
---@return any value nil when the text is not JSON
function astral.json.decode(text) end
---Encodes a value as JSON with sorted keys; `indent` spaces pretty-print it.
---@param value any
---@param options? { indent: integer? }
---@return string
function astral.json.encode(value, options) end
---Marks a table as a JSON array, so it encodes as [] even when empty.
---@param table? table
---@return table
function astral.json.array(table) end
function astral.events.off(subscription) end

astral.media = {}
---@class astral.MediaSave
---@field url string the media Discord downloaded for the save
---@field kind astral.MediaKind
---@field channel_id? astral.Snowflake the cached message that holds the media
---@field message_id? astral.Snowflake
---Runs when Discord saves media to Photos. Call `reply(url)` with an https URL to save that file instead, or `reply(nil)` to keep Discord's; needs `media.save`.
---@param callback fun(save: astral.MediaSave, reply: fun(url: string?))
function astral.media.on_save(callback) end
---@class astral.PickedMedia
---@field size? integer bytes, when the photo library knows them
---@field duration number seconds, 0 for photos
---@field type astral.PickedMediaType
---@field url? string for a video, an https URL that plays it, or nil until astral has found its file (it refreshes the screen once found)
---Describes a media picker item from its picked:// image URI.
---@param uri string
---@return astral.PickedMedia?
function astral.media.picked(uri) end
---Answers Discord's next photo picker, opened within 20 seconds, with this image instead of showing the picker.
---@param bytes string the image file's bytes, up to 1 MiB
---@param name string a png, gif, webp or jpg file name
---@return boolean offered
function astral.media.offer_pick(bytes, name) end

astral.messages = {}
function astral.messages.transform(callback) end
---@param channel astral.Snowflake
---@param message astral.Snowflake
---@param text string?
---@param options? { note?: string }
function astral.messages.set_text(channel, message, text, options) end
---@param message astral.Snowflake
---@return string?
function astral.messages.get_text(message) end
---Shows a message only you see in a channel, from astral, with Discord's "Only you can see this" footer.
---@param channel_id string
---@param message string|table text, or message fields such as content and embeds
---@return string? message_id
function astral.messages.send_local(channel_id, message) end

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
---@class astral.DiscordApiRequest
---@field path string an API path such as "/channels/1/messages/2"; the version prefix is added
---@field method? "GET"|"POST"|"PATCH"|"PUT"|"DELETE" defaults to GET
---@field body? string|table a table is sent as JSON
---Calls Discord's API as the signed-in account, with Discord's own headers. Needs `discord.api`.
---@param options astral.DiscordApiRequest
---@param callback fun(response: { ok: boolean, status: integer, body: string, error?: string })
---@return integer request
function astral.discord.request(options, callback) end
---The signed-in account's user ID.
---@return astral.Snowflake?
function astral.discord.me() end
---Every server in Discord's cache, as `{ id, name }`.
---@return { id: astral.Snowflake, name: string }[]
function astral.discord.guilds() end
---A custom emoji's name and whether it is animated, found in cached messages and reactions.
---@param id astral.Snowflake
---@return { id: astral.Snowflake, name: string, animated: boolean }?
function astral.discord.emoji(id) end
---@param channel astral.Snowflake
---@param message astral.Snowflake
---@return astral.Message?
function astral.discord.message(channel, message) end
---The full stored record of a message, without the author's private fields. Needs `discord.messages.content`.
---@param channel_id string
---@param message_id string
---@return table?
function astral.discord.raw_message(channel_id, message_id) end
---@param id astral.Snowflake
---@return astral.User?
function astral.discord.user(id) end
---@param id astral.Snowflake
---@return string?
function astral.discord.nickname(id) end
---@param id astral.Snowflake
---@return astral.Relationship?
function astral.discord.relationship(id) end

astral.storage = {}
---@alias astral.StorageValue string|number|boolean|table
---@param key string
---@param fallback? astral.StorageValue
---@return astral.StorageValue?
function astral.storage.get(key, fallback) end
---@param key string
---@param value astral.StorageValue?
function astral.storage.set(key, value) end
---@param key string
function astral.storage.remove(key) end
---@return string[]
function astral.storage.keys() end

astral.net = {}
function astral.net.request(options, callback) end
---@class astral.DiscordRequest
---@field id string pass to release or respond after returning astral.net.HOLD
---@field method string
---@field url string
---@field path string
---@field headers table<string, string> lower-case names; the token and cookies are never included
---@field body string
---@class astral.DiscordResponse
---@field status integer
---@field headers table<string, string>
---@field body string
---@class astral.Intercept
---@field path string a Lua pattern matched against the request path
---@field method? string
---@field request? fun(request: astral.DiscordRequest): table|string|nil nil sends it on; a table changes method, url, headers or body; a table with status answers locally; astral.net.HOLD waits
---@field response? fun(request: astral.DiscordRequest, response: astral.DiscordResponse): table|nil a table changes status, headers or body
---Watches and changes Discord's own requests to its API.
---@param intercept astral.Intercept
function astral.net.intercept(intercept) end
---Sends a held request on, optionally changed.
---@param id string
---@param changes? table
function astral.net.release(id, changes) end
---Answers a held request locally instead of sending it.
---@param id string
---@param response table status, headers and body
function astral.net.respond(id, response) end
---@type string
astral.net.HOLD = "hold"
function astral.net.cancel(request) end

---@param name string
---@return any
function require(name) end

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

astral.code = {}
---@class astral.HighlightResult
---@field found boolean whether the language has a grammar
---@field language string
---@field foreground integer
---@field background integer
---@field light boolean
---@field lines {[1]: string, [2]: integer, [3]: integer}[][]
---@param code string
---@param language? string fence language or file extension
---@param theme? string VS Code theme JSON
---@return astral.HighlightResult
function astral.code.highlight(code, language, theme) end
