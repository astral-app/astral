local callback
local values = {}
local listeners = {}

local function color(value)
	return {
		value = value,
		with_alpha = function(self, alpha)
			return color((alpha << 24) | (self.value & 0xFFFFFF))
		end,
		channels = function(self)
			return (self.value >> 16) & 0xFF, (self.value >> 8) & 0xFF, self.value & 0xFF, (self.value >> 24) & 0xFF
		end,
	}
end

local messages = {}
astral = {
	i18n = {
		t = function(key)
			return key
		end,
	},
	discord = {
		me = function()
			return "me"
		end,
		message = function(_, id)
			return messages[id]
		end,
	},
	Color = {
		from = function(value)
			if type(value) == "table" then
				return value
			end
			if type(value) == "number" then
				return color(value)
			end
			return color(0xFF000000 | tonumber(value:sub(2), 16))
		end,
		rgb = function(r, g, b, a)
			return color(((a or 255) << 24) | (r << 16) | (g << 8) | b)
		end,
	},
	settings = {
		values = values,
		get = function(key, fallback)
			if values[key] == nil then
				return fallback
			end
			return values[key]
		end,
		set = function(key, value)
			values[key] = value
			if listeners[key] then
				listeners[key](value, key)
			end
		end,
		on_change = function(key, listener)
			listeners[key] = listener
		end,
		option = function(value, title)
			return { value = value, title = title }
		end,
		section = function()
			return {
				choice = function(_, key, _, _, fallback)
					values[key] = fallback
				end,
				color = function(_, key, _, fallback)
					values[key] = fallback
				end,
				number = function(_, key, _, options)
					values[key] = options.default
				end,
			}
		end,
	},
	ui = {
		confirm = function(_, done)
			done(true)
		end,
		message_background = function(decorator)
			callback = decorator
		end,
	},
}

dofile(arg[1])

assert(callback)
local automatic = callback({ message_id = "m1" })
assert(
	automatic.color == 0x33000000
		and automatic.radius == 16
		and automatic.bubble
		and automatic.reply == astral.ReplyPlacement.Inside
		and automatic.name == astral.NamePlacement.Inside
		and automatic.time == astral.TimePlacement.Header
)
values.color = "#5865F2"
values.radius = 24
local picked = callback({ message_id = "m2" })
assert(picked.color.value == 0xFF5865F2 and picked.radius == 24)
values.opacity = 40
assert(callback({ message_id = "m3" }).color.value == 0x665865F2)
values.reply = astral.ReplyPlacement.Bubble
assert(callback({ message_id = "m4" }).reply == astral.ReplyPlacement.Bubble)

assert(callback({ message_id = "m5" }).padding_x == 14)
values.ownColor = "#00FF00"
assert(callback({ message_id = "m6", author_id = "me" }).color.value == 0x6600FF00)
assert(callback({ message_id = "m7", author_id = "other" }).color.value == 0x665865F2)
messages.m8 = { mentions = { { id = "me" } } }
local mentioned = callback({ message_id = "m8", channel_id = "c", author_id = "other" }).color
assert(mentioned.value ~= 0x665865F2)

assert(callback({ message_id = "m9" }).grouping == astral.BubbleGrouping.Grouped)
astral.settings.set("preset", "joined")
assert(values.grouping == astral.BubbleGrouping.Joined and values.color == "" and values.radius == 16)
assert(callback({ message_id = "m10" }).grouping == astral.BubbleGrouping.Joined)
astral.settings.set("grouping", astral.BubbleGrouping.Separate)
assert(values.preset == "rounded")
astral.settings.set("radius", 20)
assert(values.preset == "")

print("chat-bubbles ok")
