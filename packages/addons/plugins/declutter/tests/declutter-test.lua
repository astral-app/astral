local patches, gateway, intercepts = {}, nil, {}
local values = {}
astral = {
	i18n = {
		t = function(key)
			return key
		end,
	},
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
	ui = {
		patch = function(selector, fn)
			patches[selector.id or ("label:" .. selector.label)] = fn
		end,
	},
	events = {
		on = function(_, fn)
			gateway = fn
		end,
	},
	net = {
		intercept = function(definition)
			intercepts[#intercepts + 1] = definition
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
}
dofile(arg[1])
assert(patches["composer.gift"]().hidden)
assert(patches["guild.action:server_boosts"]() == nil)
values.serverBoosts = true
assert(patches["guild.action:server_boosts"]().hidden)
assert(patches["label:Quests"]() == nil)
values.quests = true
assert(patches["label:Quests"]().hidden)
assert(gateway({ type = "MESSAGE_CREATE", data = { author = { id = "1", avatar_decoration_data = {} } } }) == nil)
values.avatarDecorations = true
values.serverTags = true
local stripped = gateway({
	type = "MESSAGE_CREATE",
	data = { author = { id = "1", avatar_decoration_data = {}, primary_guild = {} } },
})
assert(stripped and stripped.author.avatar_decoration_data == nil and stripped.author.primary_guild == nil)
local response = intercepts[1].response({}, { status = 200, body = "{{author = {avatar_decoration_data = {}}}}" })
assert(response and response.body[1].author.avatar_decoration_data == nil)
values.profileFrames = true
local profile = intercepts[2].response({}, {
	status = 200,
	body = "{user = {collectibles = {}}, user_profile = {collectibles = {{type = 1}}}}",
})
assert(profile and profile.body.user_profile.collectibles == nil and profile.body.user.collectibles ~= nil)
print("declutter ok")
