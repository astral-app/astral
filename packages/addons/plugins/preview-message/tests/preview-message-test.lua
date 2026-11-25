local build, press, gateway, sent, toasted, watch
local refreshed, timers = 0, {}
local text = "hello **world**"
astral = {
	log = function() end,
	events = {
		on = function(_, fn)
			gateway = fn
		end,
	},
	discord = {
		user = function(id)
			return { id = id, username = "me" }
		end,
	},
	messages = {
		send_local = function(channel, message)
			sent = { channel = channel, message = message }
		end,
	},
	time = {
		after = function(_, fn)
			timers[#timers + 1] = fn
		end,
	},
	ui = {
		composer = function(fn)
			watch = fn
		end,
		refresh = function()
			refreshed = refreshed + 1
		end,
		composer_text = function()
			return text
		end,
		toast = function()
			toasted = true
		end,
		insert = function(_, fn, on_press)
			build, press = fn, on_press
		end,
	},
}
dofile(arg[1])
gateway({ type = "READY", data = { user = { id = "42" } } })
assert(build({}) == nil)
assert(#watch("hi") == 0)
timers[1]()
assert(refreshed == 1)
watch("hi there")
assert(#timers == 1)
local button = build({})
assert(button.action == "preview" and button.children[1].color == 0xFFABACB2)
press({ channel_id = "9" }, "preview")
assert(sent.channel == "9" and sent.message.content == text and sent.message.author.id == "42")
watch("")
assert(#timers == 2 and build({}) == nil)
text = ""
press({ channel_id = "9" }, "preview")
assert(toasted)
print("preview-message ok")
