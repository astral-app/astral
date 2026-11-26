local astral = astral
local native = {
	switch = astral.settings.switch,
	button = astral.settings.button,
	set = astral.settings.set,
	choice = astral.settings.choice,
	number = astral.settings.number,
	text = astral.settings.text,
	decorate = astral.ui.decorate,
	relabel = astral.ui.relabel,
	composer = astral.ui.composer,
	message_background = astral.ui.message_background,
	patch = astral.ui.patch,
	insert = astral.ui.insert,
	toast = astral.ui.toast,
	web_auth = astral.ui.web_auth,
	sheet = astral.ui.sheet,
	sheet_update = astral.ui.sheet_update,
	sheet_close = astral.ui.sheet_close,
	cancel_web_auth = astral.ui.cancel_web_auth,
	style_text = astral.ui.style_text,
	message_action = astral.ui.message_action,
	reaction = astral.ui.reaction,
	permission = astral.discord.permission,
	enums = astral.enums,
}
astral.enums = nil

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

for name, members in pairs(native.enums) do
	astral[name] = astral.enum(name, members)
end

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
	self.text = text ~= nil and tostring(text) or ""
	self.icon = options.icon
	self.tint = options.tint and Color.from(options.tint) or nil
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
	if self.icon then
		return { icon = self.icon, text = self.text, tint = self.tint and self.tint.value or nil }
	end
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

local BASE64_ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
local BASE64_VALUES = {}
for index = 1, #BASE64_ALPHABET do
	BASE64_VALUES[BASE64_ALPHABET:byte(index)] = index - 1
end
BASE64_VALUES[("-"):byte()] = 62
BASE64_VALUES[("_"):byte()] = 63

astral.base64 = {
	encode = function(text)
		local out = {}
		for index = 1, #text, 3 do
			local a, b, c = text:byte(index, index + 2)
			local value = (a << 16) | ((b or 0) << 8) | (c or 0)
			out[#out + 1] = BASE64_ALPHABET:sub((value >> 18) + 1, (value >> 18) + 1)
				.. BASE64_ALPHABET:sub(((value >> 12) & 63) + 1, ((value >> 12) & 63) + 1)
				.. (b and BASE64_ALPHABET:sub(((value >> 6) & 63) + 1, ((value >> 6) & 63) + 1) or "=")
				.. (c and BASE64_ALPHABET:sub((value & 63) + 1, (value & 63) + 1) or "=")
		end
		return table.concat(out)
	end,
	decode = function(text)
		if type(text) ~= "string" then
			return nil
		end
		local out, bits, count = {}, 0, 0
		for index = 1, #text do
			local byte = text:byte(index)
			local value = BASE64_VALUES[byte]
			if value then
				bits = (bits << 6) | value
				count = count + 6
				if count >= 8 then
					count = count - 8
					out[#out + 1] = string.char((bits >> count) & 255)
				end
			elseif byte ~= 61 and byte ~= 10 and byte ~= 13 then
				return nil
			end
		end
		return table.concat(out)
	end,
}

local DISCORD_EPOCH = 1420070400000
local SNOWFLAKE_TIME_SHIFT = 22

local function snowflake_integer(id)
	local value = math.tointeger(tonumber(id))
	if not value or value < 0 then
		return nil
	end
	return value
end

astral.snowflake = {
	EPOCH = DISCORD_EPOCH,
	timestamp = function(id)
		local value = snowflake_integer(id)
		return value and (value >> SNOWFLAKE_TIME_SHIFT) + DISCORD_EPOCH or nil
	end,
	from_time = function(milliseconds)
		local value = math.tointeger(milliseconds)
		if not value or value < DISCORD_EPOCH then
			return nil
		end
		return tostring((value - DISCORD_EPOCH) << SNOWFLAKE_TIME_SHIFT)
	end,
	is_valid = function(id)
		return type(id) == "string" and id:match("^%d+$") ~= nil and snowflake_integer(id) ~= nil
	end,
	compare = function(left, right)
		local a, b = snowflake_integer(left), snowflake_integer(right)
		if not a or not b then
			return nil
		end
		return a < b and -1 or (a > b and 1 or 0)
	end,
}

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

function astral.settings.color(key, title, default, options)
	local merged = { color = true }
	for name, value in pairs(options or {}) do
		merged[name] = value
	end
	values[key] = native.text(key, title, default or "", merged)
	return values[key]
end

function astral.settings.button(key, title, callback, options)
	native.button(key, title, callback, options)
end

function astral.settings.set(key, value)
	native.set(key, value)
	if values[key] ~= nil and values[key] ~= value then
		astral.on_setting(key, value)
	end
end

function astral.settings.on_change(key, listener)
	listeners[key] = listeners[key] or {}
	table.insert(listeners[key], listener)
end

local Section = astral.class("Section")

function Section:init(title)
	self.title = title
end

function Section:options(extra)
	local merged = { section = self.title }
	if type(extra) == "table" then
		for name, value in pairs(extra) do
			merged[name] = value
		end
	else
		merged.subtitle = extra
	end
	return merged
end

function Section:switch(key, title, default, options)
	return astral.settings.switch(key, title, default, self:options(options))
end

function Section:button(key, title, callback, options)
	return astral.settings.button(key, title, callback, self:options(options))
end

function Section:choice(key, title, choices, default, options)
	return astral.settings.choice(key, title, choices, default, self:options(options))
end

function Section:color(key, title, default, options)
	return astral.settings.color(key, title, default, self:options(options))
end

function Section:number(key, title, options)
	return astral.settings.number(key, title, self:options(options))
end

function Section:text(key, title, default, options)
	return astral.settings.text(key, title, default, self:options(options))
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

local function wrap_node(node)
	rawset(node, "_cache", {})
	return setmetatable(node, Node)
end

local function build_pills(result)
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
		return build_pills(decorator(wrap_node(node)))
	end)
end

function astral.ui.message_background(decorator)
	return native.message_background(function(node)
		local style = decorator(wrap_node(node))
		if type(style) ~= "table" or style.color == nil then
			return nil
		end
		return {
			color = astral.Color.from(style.color).value,
			radius = style.radius or 0,
			bubble = style.bubble == true,
			reply = style.reply,
			name = style.name,
			time = style.time,
		}
	end)
end

function astral.ui.message_action(label, callback, options)
	local wrapped = options
	if options and options.when then
		wrapped = {
			icon = options.icon,
			when = function(node)
				return options.when(wrap_node(node)) == true
			end,
		}
	end
	return native.message_action(label, function(node)
		return callback(wrap_node(node))
	end, wrapped)
end

function astral.ui.composer(formatter)
	return native.composer(formatter)
end

function astral.ui.sheet(spec, handler)
	local id = native.sheet(spec, function(action, value)
		if handler then
			handler(action, value)
		end
	end)
	return {
		id = id,
		update = function(_, next_spec)
			native.sheet_update(id, next_spec)
		end,
		close = function()
			native.sheet_close(id)
		end,
	}
end

astral.ui.sheet_update = nil
astral.ui.sheet_close = nil

function astral.ui.toast(title, message)
	return native.toast(title, message)
end

function astral.ui.web_auth(url, callback_url, callback)
	return native.web_auth(url, callback_url, callback)
end

function astral.ui.cancel_web_auth(request)
	return native.cancel_web_auth(request)
end

function astral.ui.style_text(styler)
	return native.style_text(function(node)
		local style = styler(wrap_node(node))
		if style == nil then
			return nil
		end
		if type(style) ~= "table" or astral.Color.is(style) then
			style = { color = style }
		end
		return {
			color = style.color ~= nil and astral.Color.from(style.color).value or nil,
			image = style.image,
			round = style.round,
			hide_at = style.hide_at,
			after = style.after,
		}
	end)
end

function astral.ui.reaction(decorator)
	return native.reaction(function(node)
		return decorator(wrap_node(node))
	end)
end

function astral.ui.patch(selector, patcher)
	local id, kind = selector, nil
	if type(selector) == "table" then
		id, kind = selector.id, selector.type
	end
	return native.patch(id, kind, function(node)
		local result = patcher(node)
		if type(result) ~= "table" then
			return result
		end
		local patch = {}
		for field, value in pairs(result) do
			if field:find("color$") or field == "background" then
				patch[field] = astral.Color.from(value).value
			elseif field == "gradient" and type(value) == "table" then
				local colors = {}
				for index, color in ipairs(value.colors or {}) do
					colors[index] = astral.Color.from(color).value
				end
				patch.gradient = { angle = value.angle, colors = colors }
			else
				patch[field] = value
			end
		end
		return patch
	end)
end

local function insert_spec(spec)
	if type(spec) ~= "table" then
		return spec
	end
	local converted = {}
	for field, value in pairs(spec) do
		if field == "color" or field == "background" then
			converted[field] = astral.Color.from(value).value
		elseif field == "children" or (type(field) == "number" and type(value) == "table") then
			local items = {}
			for index, child in ipairs(field == "children" and value or { value }) do
				items[index] = insert_spec(child)
			end
			converted[field] = field == "children" and items or items[1]
		else
			converted[field] = value
		end
	end
	return converted
end

function astral.ui.insert(selector, build, on_press)
	local id, kind = selector, nil
	if type(selector) == "table" then
		id, kind = selector.id, selector.type
	end
	return native.insert(id, kind, function(node)
		return insert_spec(build(wrap_node(node)))
	end, on_press and function(node, action)
		on_press(wrap_node(node), action)
	end)
end

function astral.ui.relabel(target, relabeler)
	return native.relabel(target, function(node)
		return relabeler(wrap_node(node))
	end)
end
