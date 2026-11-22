local function englishStrings()
	local strings = {}
	local folder = arg[1]:match("(.*/)") or "./"
	local file = io.open(folder .. "i18n/en-US.toml")
	if file then
		for line in file:lines() do
			local key, value = line:match('^([%w_]+) = "(.*)"$')
			if key then
				strings[key] = value
			end
		end
		file:close()
	end
	return strings
end

local english = englishStrings()

local relabel = nil
local stored = {}
local messages = {
	["c:m1"] = { type = 0, author = { id = "u1", username = "kode", global_name = "Kode Toad" } },
	["c:m2"] = {
		type = 0,
		author = { id = "u2", username = "rae", global_name = "Rae" },
		reference = { channel_id = "c", message_id = "m1" },
	},
	["c:m3"] = { type = 24, author = { id = "u3", username = "automod" } },
	["c:m4"] = { type = 0, author = { id = "u4", username = "same", global_name = "same" } },
}
local nicknames = { u2 = "Rae Friend" }

astral = {
	log = function() end,
	i18n = {
		t = function(key)
			return english[key] or key
		end,
	},
	settings = {
		switch = function(key, _, default)
			if stored[key] == nil then
				return default
			end
			return stored[key]
		end,
		choice = function(key, _, _, default)
			return stored[key] or default
		end,
	},
	ui = {
		relabel = function(_, fn)
			relabel = fn
		end,
	},
	discord = {
		message = function(channel, message)
			return messages[channel .. ":" .. message]
		end,
		nickname = function(user)
			return nicknames[user]
		end,
	},
}

dofile(arg[1])

local function label(kind, message, text)
	return relabel({ kind = kind, text = text, channel_id = "c", message_id = message })
end

local function expect(name, actual, wanted)
	assert(actual == wanted, name .. ": expected " .. tostring(wanted) .. ", got " .. tostring(actual))
end

expect("nick then user", label("author", "m1", "Kode Toad"), "Kode Toad (@kode)")
expect("friend nickname", label("author", "m2", "Rae"), "Rae Friend (@rae)")
expect("automod untouched", label("author", "m3", "AutoMod"), nil)
expect("same name untouched", label("author", "m4", "same"), "same")
expect("replies off", label("reply", "m2", "Kode Toad"), nil)
astral.on_setting("inReplies", true)
expect("reply author", label("reply", "m2", "@Kode Toad"), "@Kode Toad (@kode)")
astral.on_setting("mode", "user-nick")
expect("user then nick", label("author", "m1", "Kode Toad"), "@kode (Kode Toad)")
astral.on_setting("mode", "user")
expect("user only", label("author", "m1", "Kode Toad"), "@kode")
astral.on_setting("displayNames", true)
astral.on_setting("friendNicknames", false)
astral.on_setting("mode", "nick-user")
expect("global names", label("author", "m2", "Rae"), "Rae")
print("show-me-your-name ok")
