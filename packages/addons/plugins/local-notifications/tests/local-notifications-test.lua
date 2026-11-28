local handlers, gateway, notified, alive = {}, nil, {}, nil
local pushed, mode, posted = true, "automatic", nil
astral = {
	i18n = {
		t = function(key)
			local strings = { ATTACHMENT = "Sent an attachment", NEW_MESSAGE = "Sent a message", SOMEONE = "Someone" }
			return strings[key] or key
		end,
	},
	log = function() end,
	settings = {
		get = function(key, fallback)
			if key == "mode" then
				return mode
			end
			return fallback
		end,
		option = function(value)
			return value
		end,
		section = function()
			return { switch = function() end, choice = function() end }
		end,
	},
	discord = {
		guild = function()
			return { name = "Cave" }
		end,
		request = function(options)
			posted = options
		end,
	},
	background = {
		keep_alive = function(on)
			alive = on
			return true
		end,
		push_registered = function()
			return pushed
		end,
	},
	ui = {
		notify = function(title, body)
			notified[#notified + 1] = title .. ": " .. body
		end,
	},
	events = {
		on = function(event, fn)
			if event == "gateway" then
				gateway = fn
			else
				handlers[event] = fn
			end
		end,
	},
}
dofile(arg[1])
gateway({ type = "READY", data = { user = { id = "me" } } })
handlers["app.background"]()
gateway({ type = "MESSAGE_CREATE", data = { id = "0", author = { id = "a", username = "ann" }, content = "pushed" } })
assert(#notified == 0 and alive == nil)
handlers["app.foreground"]()
pushed = false
gateway({ type = "MESSAGE_CREATE", data = { id = "1", author = { id = "a", username = "ann" }, content = "hi" } })
assert(#notified == 0)
handlers["app.background"]()
assert(alive == true)
gateway({ type = "MESSAGE_CREATE", data = { id = "2", author = { id = "a", username = "ann" }, content = "dm" } })
gateway({
	type = "MESSAGE_CREATE",
	data = {
		id = "3",
		guild_id = "g",
		author = { id = "b", global_name = "Bo" },
		content = "hey",
		mentions = { { id = "me" } },
	},
})
gateway({
	type = "MESSAGE_CREATE",
	data = { id = "4", guild_id = "g", author = { id = "b" }, content = "noise", mentions = {} },
})
gateway({ type = "MESSAGE_CREATE", data = { id = "5", author = { id = "me" }, content = "mine" } })
assert(#notified == 2 and notified[1] == "ann: dm" and notified[2] == "Bo in Cave: hey")
handlers["app.foreground"]()
assert(alive == false)
mode = "always"
pushed = true
handlers["app.background"]()
assert(alive == true)
handlers["notification.reply"]({ channel_id = "c", message_id = "2", text = "thanks" })
assert(posted.method == "POST" and posted.path == "/channels/c/messages" and posted.body.content == "thanks")
assert(posted.body.message_reference.message_id == "2")
print("local-notifications ok")
