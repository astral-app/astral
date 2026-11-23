local callbacks = {}
local values = {}

astral = {
	settings = {
		values = values,
		section = function()
			return {
				text = function(_, key, _, fallback)
					values[key] = fallback
				end,
				switch = function(_, key, _, fallback)
					values[key] = fallback
				end,
			}
		end,
	},
	events = {
		on = function(event, callback)
			callbacks[event] = callback
			return 1
		end,
	},
}

dofile(arg[1])

local filter = callbacks["message.filter"]
assert(filter)
values.users = " 123456789012345678\ninvalid\n"
values.keywords = "spoiler\nfree nitro"
assert(filter({ author_id = "123456789012345678", content = "hello" }))
assert(filter({ author_id = "234567890123456789", content = "A SPOILER ahead" }))
assert(filter({ author_id = "234567890123456789", content = "join free nitro now" }))
assert(not filter({ author_id = "234567890123456789", content = "ordinary message" }))
assert(filter({
	author_id = "234567890123456789",
	content = "ordinary message",
	referenced_author_id = "123456789012345678",
}))
values.replies = false
assert(not filter({
	author_id = "234567890123456789",
	content = "ordinary message",
	referenced_author_id = "123456789012345678",
}))
assert(not filter({ from_me = true, author_id = "123456789012345678", content = "spoiler" }))

print("enhanced-blocking ok")
