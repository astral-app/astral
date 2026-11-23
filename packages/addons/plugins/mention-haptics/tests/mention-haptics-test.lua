local callback
local haptics = {}
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
				switch = function(_, key, _, fallback)
					values[key] = fallback
				end,
			}
		end,
	},
	ui = {
		haptic = function(strength)
			haptics[#haptics + 1] = strength
		end,
	},
	events = {
		on = function(event, handler)
			assert(event == "message.create")
			callback = handler
			return 1
		end,
	},
}

dofile(arg[1])

assert(callback)
callback({ message_id = "m1", from_me = false, mentioned = true, direct = false, open = false })
assert(haptics[1] == "medium")
callback({ message_id = "m2", from_me = true, mentioned = true, direct = false, open = false })
callback({ message_id = "m3", from_me = false, mentioned = true, direct = false, open = true })
assert(#haptics == 1)
values.openChannel = true
callback({ message_id = "m4", from_me = false, mentioned = true, direct = false, open = true })
assert(#haptics == 2)
callback({ message_id = "m4", from_me = false, mentioned = true, direct = false, open = false })
assert(#haptics == 2)
callback({ message_id = "m5", from_me = false, mentioned = false, direct = true, open = false })
assert(#haptics == 3)
values.directMessages = false
callback({ message_id = "m6", from_me = false, mentioned = false, direct = true, open = false })
assert(#haptics == 3)
values.strength = "heavy"
callback({ message_id = "m7", from_me = false, mentioned = true, direct = false, open = false })
assert(haptics[4] == "heavy")

print("mention-haptics ok")
