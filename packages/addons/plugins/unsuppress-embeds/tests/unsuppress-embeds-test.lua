local actions, edits, toasts = {}, {}, {}
local strings = {
	SUPPRESS_EMBEDS = "Suppress Embeds",
	UNSUPPRESS_EMBEDS = "Unsuppress Embeds",
	CHANGE_FAILED = "{action} failed: {reason}",
	CHANGE_REFUSED = "Discord refused the change.",
}
local messages = {
	["1"] = { id = "1", flags = 0, embeds = 2, author = { id = "100" } },
	["2"] = { id = "2", flags = 4 | 16, embeds = 0, author = { id = "100" } },
	["3"] = { id = "3", flags = 0, embeds = 0, author = { id = "100" } },
	["4"] = { id = "4", flags = 0, embeds = 1, author = { id = "200" } },
}
local granted = 0
astral = {
	log = function() end,
	settings = {},
	i18n = {
		t = function(key)
			return strings[key] or key
		end,
	},
	json = {
		decode = function(text)
			return text == '{"message":"Missing Permissions"}' and { message = "Missing Permissions" } or nil
		end,
	},
	discord = {
		me = function()
			return "100"
		end,
		permissions = function()
			return granted
		end,
		request = function(options, callback)
			edits[#edits + 1] = { options = options, callback = callback }
			return #edits
		end,
	},
	Permission = { ADMINISTRATOR = 8, MANAGE_MESSAGES = 8192, EMBED_LINKS = 16384 },
	permissions = {
		any = function(bits, ...)
			for index = 1, select("#", ...) do
				if bits & select(index, ...) ~= 0 then
					return true
				end
			end
			return false
		end,
	},
	ui = {
		message_action = function(title, fn, options)
			actions[title] = { fn = fn, options = options }
		end,
		toast = function(title, text)
			toasts[#toasts + 1] = { title = title, text = text }
		end,
	},
}
dofile(arg[1])

local function node(message, guild)
	return {
		channel_id = "9",
		message_id = message,
		guild_id = guild,
		message = function()
			return messages[message]
		end,
	}
end

local suppress, unsuppress = actions["Suppress Embeds"], actions["Unsuppress Embeds"]
assert(suppress.options.icon == "res://ic_mana_eye_slash" and unsuppress.options.icon == "res://ic_mana_eye")
assert(suppress.options.danger == true and not unsuppress.options.danger)

assert(suppress.options.when(node("1")) and not unsuppress.options.when(node("1")))
assert(unsuppress.options.when(node("2")) and not suppress.options.when(node("2")))
assert(not suppress.options.when(node("3")) and not unsuppress.options.when(node("3")))
assert(not suppress.options.when(node("4")))

assert(not suppress.options.when(node("1", "5")))
granted = astral.Permission.EMBED_LINKS
assert(suppress.options.when(node("1", "5")))
assert(not suppress.options.when(node("4", "5")))
granted = astral.Permission.MANAGE_MESSAGES
assert(suppress.options.when(node("4", "5")))

suppress.fn(node("1"))
assert(edits[1].options.method == "PATCH" and edits[1].options.path == "/channels/9/messages/1")
assert(edits[1].options.body.flags == 4)
unsuppress.fn(node("2"))
assert(edits[2].options.body.flags == 16)

edits[1].callback({ ok = true, status = 200, body = "{}" })
assert(#toasts == 0)
edits[2].callback({ ok = false, status = 403, body = '{"message":"Missing Permissions"}' })
assert(toasts[1].text == "Unsuppress Embeds failed: Missing Permissions")
edits[2].callback({ ok = false, status = 0, body = "" })
assert(toasts[2].text == "Unsuppress Embeds failed: Discord refused the change.")
