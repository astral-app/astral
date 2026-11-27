local intercept
local patcher
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
	ui = {
		patch = function(_, fn)
			patcher = fn
		end,
	},
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
values.shouldPingListed = true
values.userList = "42"
local listed = intercept.request({ path = path, headers = { ["content-type"] = "application/json" }, body = reply })
assert(listed.body.allowed_mentions.replied_user == true)
values.shouldPingListed = false

local shown = patcher({ node = 7, checked = true })
assert(shown.label.text == "OFF")
assert(patcher({ node = 7, checked = false }).label.text == "ON")
local flipped = intercept.request({ path = path, headers = { ["content-type"] = "application/json" }, body = reply })
assert(flipped.body.allowed_mentions.replied_user == true)
assert(patcher({ node = 8, checked = false }) == nil)
assert(patcher({ node = 8 }) == nil)
print("no-reply-mention ok")
