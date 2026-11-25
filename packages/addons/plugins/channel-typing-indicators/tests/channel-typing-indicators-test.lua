local gateway, insert
local now = 1000
local values = {}
astral = {
	log = function() end,
	settings = {
		get = function(key, fallback)
			if values[key] == nil then
				return fallback
			end
			return values[key]
		end,
		section = function()
			return { switch = function() end }
		end,
	},
	time = {
		now_ms = function()
			return now
		end,
		after = function()
			return 1
		end,
		cancel = function() end,
	},
	events = {
		on = function(_, fn)
			gateway = fn
		end,
	},
	discord = {
		relationship = function(id)
			return id == "blocked" and "blocked" or nil
		end,
	},
	ui = {
		insert = function(_, fn)
			insert = fn
		end,
		refresh = function() end,
	},
}
dofile(arg[1])
gateway({ type = "READY", data = { user = { id = "me" } } })
gateway({ type = "TYPING_START", data = { channel_id = "c1", user_id = "a" } })
gateway({ type = "TYPING_START", data = { channel_id = "c1", user_id = "b" } })
gateway({ type = "TYPING_START", data = { channel_id = "c2", user_id = "me" } })
gateway({ type = "TYPING_START", data = { channel_id = "c3", user_id = "blocked" } })
assert(insert({ key = "c1" }).text == "••• 2")
assert(insert({ key = "c2" }) == nil and insert({ key = "c3" }) == nil)
gateway({ type = "MESSAGE_CREATE", data = { channel_id = "c1", author = { id = "a" } } })
assert(insert({ key = "c1" }).text == "•••")
now = now + 11000
assert(insert({ key = "c1" }) == nil)
print("channel-typing-indicators ok")
