local build, press, gateway, sent, toasted
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
	ui = {
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
assert(build({}).action == "preview")
press({ channel_id = "9" }, "preview")
assert(sent.channel == "9" and sent.message.content == text and sent.message.author.id == "42")
text = ""
press({ channel_id = "9" }, "preview")
assert(toasted)
print("preview-message ok")
