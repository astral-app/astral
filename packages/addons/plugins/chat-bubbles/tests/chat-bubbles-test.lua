local callback
local values = {}

local function color(value)
	return {
		value = value,
		with_alpha = function(self, alpha)
			return color((alpha << 24) | (self.value & 0xFFFFFF))
		end,
	}
end

astral = {
	Color = {
		from = function(text)
			return color(0xFF000000 | tonumber(text:sub(2), 16))
		end,
	},
	settings = {
		values = values,
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

print("chat-bubbles ok")
