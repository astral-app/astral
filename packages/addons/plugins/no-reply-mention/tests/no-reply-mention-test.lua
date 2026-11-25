local intercept
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
			return { switch = function() end, text = function() end }
		end,
	},
	net = {
		intercept = function(definition)
			intercept = definition
		end,
	},
	json = {
		decode = function(text)
			return load("return " .. text)()
		end,
		encode = function(value)
			return value
		end,
	},
	discord = {
		permission = {},
		message = function(_, id)
			return id == "5" and { author = { id = "42" } } or nil
		end,
	},
	ui = {},
	i18n = {
		t = function(key)
			return key
		end,
	},
}
dofile(arg[1])
local reply = '{content = "hi", message_reference = {message_id = "5"}}'
local path = "/api/v9/channels/1/messages"
local changed = intercept.request({ path = path, headers = { ["content-type"] = "application/json" }, body = reply })
assert(changed.body.allowed_mentions.replied_user == false)
assert(
	intercept.request({ path = path, headers = { ["content-type"] = "application/json" }, body = '{content = "hi"}' })
		== nil
)
values.only_listed = true
values.listed = "42"
assert(intercept.request({ path = path, headers = { ["content-type"] = "application/json" }, body = reply }) == nil)
print("no-reply-mention ok")
