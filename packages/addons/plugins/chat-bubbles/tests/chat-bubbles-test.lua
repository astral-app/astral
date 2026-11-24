local callback
local values = {}

astral = {
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
local neutral = callback({ message_id = "m1" })
assert(neutral.color == 0x24FFFFFF and neutral.radius == 12 and neutral.bubble and neutral.reply == "inside")
values.color = "blue"
values.radius = 24
local blue = callback({ message_id = "m2" })
assert(blue.color == 0x665865F2 and blue.radius == 24)
values.color = "unknown"
local fallback = callback({ message_id = "m3" })
assert(fallback.color == 0x24FFFFFF and fallback.radius == 24)
values.reply = "bubble"
assert(callback({ message_id = "m4" }).reply == "bubble")

print("chat-bubbles ok")
