local astral = astral
local native = {
	switch = astral.settings.switch,
	choice = astral.settings.choice,
	number = astral.settings.number,
	text = astral.settings.text,
	decorate = astral.ui.decorate,
	relabel = astral.ui.relabel,
	mention = astral.ui.mention,
	message_color = astral.ui.message_color,
	name_color = astral.ui.name_color,
	typing = astral.ui.typing,
	reaction = astral.ui.reaction,
	permission = astral.discord.permission,
}

function astral.enum(name, members)
	local frozen = {}
	for key, value in pairs(members) do
		frozen[key] = value
	end
	return setmetatable({}, {
		__name = name,
		__index = function(_, key)
			local value = frozen[key]
			if value == nil then
				error(name .. " has no member '" .. tostring(key) .. "'", 2)
			end
			return value
		end,
		__newindex = function()
			error(name .. " is read-only", 2)
		end,
		__pairs = function()
			return next, frozen, nil
		end,
		__tostring = function()
			return "enum " .. name
		end,
	})
end

function astral.class(name, parent)
	local class = {}
	class.__index = class
	class.__name = name
	class.super = parent
	class.__tostring = function(instance)
		if instance.describe then
			return name .. "(" .. instance:describe() .. ")"
		end
		return name
	end

	function class.new(...)
		local instance = setmetatable({}, class)
		if instance.init then
			instance:init(...)
		end
		return instance
	end

	function class.is(value)
		local meta = getmetatable(value)
		while meta do
			if meta == class then
				return true
			end
			meta = meta.super
		end
		return false
	end

	return setmetatable(class, {
		__index = parent,
		__call = function(_, ...)
			return class.new(...)
		end,
	})
end

astral.Target = astral.enum("Target", {
	MessageAuthor = "message.author",
	ChannelName = "channel.name",
	MemberName = "member.name",
	ComposerPlaceholder = "composer.placeholder",
	ProfileName = "profile.name",
})

astral.NodeKind = astral.enum("NodeKind", {
	Author = "author",
	Reply = "reply",
	Channel = "channel",
	Member = "member",
	Composer = "composer",
	Profile = "profile",
})

astral.Shape = astral.enum("Shape", { Pill = "pill", Op = "op" })

astral.MessageType = astral.enum("MessageType", {
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
})

astral.Permission = astral.enum("Permission", native.permission)
astral.discord.permission = nil

local function combine(...)
	local bits = 0
	for index = 1, select("#", ...) do
		bits = bits | (select(index, ...) or 0)
	end
	return bits
end

astral.permissions = {
	combine = combine,
	any = function(granted, ...)
		return granted ~= nil and granted & combine(...) ~= 0
	end,
	all = function(granted, ...)
		local wanted = combine(...)
		return granted ~= nil and granted & wanted == wanted
	end,
}

local Color = astral.class("Color")

local function channel(value)
	return math.max(0, math.min(255, math.floor(value + 0.5)))
end

function Color:init(argb)
	self.value = math.tointeger(argb) or 0
end

function Color.argb(value)
	return Color.new(value)
end

function Color.rgb(red, green, blue, alpha)
	return Color.new((channel(alpha or 255) << 24) | (channel(red) << 16) | (channel(green) << 8) | channel(blue))
end

function Color.hex(text)
	local digits = tostring(text):gsub("^#", "")
	if #digits == 3 then
		digits = digits:gsub(".", "%0%0")
	end
	if #digits == 6 then
		digits = "FF" .. digits
	end
	local value = #digits == 8 and tonumber(digits, 16)
	if not value then
		error("Color.hex: '" .. tostring(text) .. "' is not #RGB, #RRGGBB or #AARRGGBB", 2)
	end
	return Color.new(value)
end

function Color.from(value)
	if Color.is(value) then
		return value
	end
	if type(value) == "string" then
		return Color.hex(value)
	end
	return Color.new(value)
end

function Color:channels()
	local value = self.value
	return (value >> 16) & 0xFF, (value >> 8) & 0xFF, value & 0xFF, (value >> 24) & 0xFF
end

function Color:opaque()
	return Color.new(0xFF000000 | (self.value & 0xFFFFFF))
end

function Color:with_alpha(alpha)
	return Color.new((channel(alpha) << 24) | (self.value & 0xFFFFFF))
end

function Color:is_transparent()
	return self.value & 0xFFFFFF == 0
end

function Color:luminance()
	local total = 0
	local red, green, blue = self:channels()
	for index, value in ipairs({ red, green, blue }) do
		local linear = value / 255
		linear = linear <= 0.04045 and linear / 12.92 or ((linear + 0.055) / 1.055) ^ 2.4
		total = total + linear * ({ 0.2126, 0.7152, 0.0722 })[index]
	end
	return total
end

function Color:describe()
	return string.format("#%08X", self.value & 0xFFFFFFFF)
end

Color.__eq = function(left, right)
	return left.value == right.value
end

Color.Black = Color.new(0xFF000000)
Color.White = Color.new(0xFFFFFFFF)
Color.Blurple = Color.new(0xFF5865F2)

function Color.contrasting(backgrounds)
	local white, black = math.huge, math.huge
	for _, background in ipairs(backgrounds) do
		local luminance = Color.from(background):luminance()
		white = math.min(white, 1.05 / (luminance + 0.05))
		black = math.min(black, (luminance + 0.05) / 0.05)
	end
	return black > white and Color.Black or Color.White
end

astral.Color = Color

local Pill = astral.class("Pill")

function Pill:init(text, options)
	options = options or {}
	self.text = tostring(text)
	self.shape = options.shape or astral.Shape.Pill
	self.stops = {}
	for _, stop in ipairs(options.colors or { options.color }) do
		self.stops[#self.stops + 1] = Color.from(stop)
	end
	self.text_color = options.text_color and Color.from(options.text_color) or nil
end

function Pill:color(color)
	self.stops = { Color.from(color) }
	return self
end

function Pill:gradient(colors)
	self.stops = {}
	for _, stop in ipairs(colors) do
		self.stops[#self.stops + 1] = Color.from(stop)
	end
	return self
end

function Pill:text_colored(color)
	self.text_color = Color.from(color)
	return self
end

function Pill:shaped(shape)
	self.shape = shape
	return self
end

function Pill:describe()
	return self.text
end

function Pill:build()
	local stops = #self.stops > 0 and self.stops or { Color.Blurple }
	local built = {
		text = self.text,
		color = stops[1].value,
		text_color = (self.text_color or Color.contrasting(stops)).value,
		shape = self.shape,
	}
	if #stops >= 2 then
		built.colors = {}
		for index, stop in ipairs(stops) do
			built.colors[index] = stop.value
		end
	end
	return built
end

astral.Pill = Pill

astral.list = {
	map = function(items, transform)
		local result = {}
		for index, item in ipairs(items) do
			result[index] = transform(item, index)
		end
		return result
	end,
	filter = function(items, keep)
		local result = {}
		for index, item in ipairs(items) do
			if keep(item, index) then
				result[#result + 1] = item
			end
		end
		return result
	end,
	find = function(items, matches)
		for index, item in ipairs(items) do
			if matches(item, index) then
				return item, index
			end
		end
		return nil
	end,
	contains = function(items, wanted)
		for _, item in ipairs(items) do
			if item == wanted then
				return true
			end
		end
		return false
	end,
	push_unique = function(items, item)
		for _, existing in ipairs(items) do
			if existing == item then
				return items
			end
		end
		items[#items + 1] = item
		return items
	end,
}

local values = {}
local listeners = {}

astral.settings.values = values

function astral.settings.switch(key, title, default, options)
	values[key] = native.switch(key, title, default, options)
	return values[key]
end

function astral.settings.choice(key, title, choices, default, options)
	values[key] = native.choice(key, title, choices, default, options)
	return values[key]
end

function astral.settings.number(key, title, options)
	values[key] = native.number(key, title, options)
	return values[key]
end

function astral.settings.text(key, title, default, options)
	values[key] = native.text(key, title, default, options)
	return values[key]
end

function astral.settings.on_change(key, listener)
	listeners[key] = listeners[key] or {}
	table.insert(listeners[key], listener)
end

local Section = astral.class("Section")

function Section:init(title)
	self.title = title
end

function Section:switch(key, title, default, subtitle)
	return astral.settings.switch(key, title, default, { subtitle = subtitle, section = self.title })
end

function Section:choice(key, title, choices, default, subtitle)
	return astral.settings.choice(key, title, choices, default, { subtitle = subtitle, section = self.title })
end

function Section:number(key, title, options)
	local merged = { section = self.title }
	for name, value in pairs(options or {}) do
		merged[name] = value
	end
	return astral.settings.number(key, title, merged)
end

function Section:text(key, title, default, options)
	local merged = { section = self.title }
	for name, value in pairs(options or {}) do
		merged[name] = value
	end
	return astral.settings.text(key, title, default, merged)
end

function Section:describe()
	return tostring(self.title)
end

function astral.settings.section(title)
	return Section.new(title)
end

function astral.settings.option(value, title, subtitle)
	return { value = value, title = title, subtitle = subtitle }
end

astral.state = {}

function astral.on_setting(key, value)
	values[key] = value
	for _, listener in ipairs(listeners[key] or {}) do
		listener(value, key)
	end
end

local Node = {}
Node.__index = Node

local function cached(node, name, load)
	local cache = rawget(node, "_cache")
	if cache[name] == nil then
		cache[name] = load() or false
	end
	return cache[name] or nil
end

function Node:message()
	return cached(self, "message", function()
		return self.channel_id and self.message_id and astral.discord.message(self.channel_id, self.message_id)
	end)
end

function Node:guild()
	return cached(self, "guild", function()
		return self.guild_id and astral.discord.guild(self.guild_id)
	end)
end

function Node:member()
	return cached(self, "member", function()
		return self.guild_id and self.user_id and astral.discord.member(self.guild_id, self.user_id)
	end)
end

function Node:user()
	return cached(self, "user", function()
		return self.user_id and astral.discord.user(self.user_id)
	end)
end

function Node:permissions()
	return cached(self, "permissions", function()
		return self.guild_id
			and self.user_id
			and astral.discord.permissions(self.guild_id, self.user_id, self.channel_id)
	end)
end

function Node:is_bot()
	local message = self:message()
	return message ~= nil and message.author ~= nil and message.author.bot == true
end

local function wrapNode(node)
	rawset(node, "_cache", {})
	return setmetatable(node, Node)
end

local function buildPills(result)
	if result == nil then
		return nil
	end
	if Pill.is(result) then
		result = { result }
	end
	local built = {}
	for _, pill in ipairs(result) do
		if Pill.is(pill) then
			built[#built + 1] = pill:build()
		elseif pill then
			built[#built + 1] = Pill.new(pill.text, pill):build()
		end
	end
	return #built > 0 and built or nil
end

function astral.ui.decorate(target, decorator)
	return native.decorate(target, function(node)
		return buildPills(decorator(wrapNode(node)))
	end)
end

function astral.ui.mention(decorator)
	return native.mention(function(node)
		return decorator(wrapNode(node))
	end)
end

function astral.ui.message_color(colorizer)
	return native.message_color(function(node)
		local color = colorizer(wrapNode(node))
		if color == nil then
			return nil
		end
		return astral.Color.from(color).value
	end)
end

function astral.ui.name_color(colorizer)
	return native.name_color(function(node)
		local color = colorizer(wrapNode(node))
		if color == nil then
			return nil
		end
		return astral.Color.from(color).value
	end)
end

function astral.ui.typing(decorator)
	return native.typing(function(node)
		return decorator(wrapNode(node))
	end)
end

function astral.ui.reaction(decorator)
	return native.reaction(function(node)
		return decorator(wrapNode(node))
	end)
end

function astral.ui.relabel(target, relabeler)
	return native.relabel(target, function(node)
		return relabeler(wrapNode(node))
	end)
end
