local intercept, rewrite, released
local intercepts = {}
local network
astral = {
	log = function() end,
	i18n = {
		t = function()
			return "{mod}: {name}"
		end,
	},
	discord = {
		me = function()
			return "42"
		end,
	},
	json = {
		decode = function(text)
			if text == "catalog" then
				return { badges = { { badge_id = 22 } } }
			end
			return { badges = {} }
		end,
		encode = function(value)
			return value
		end,
		array = function()
			return {}
		end,
	},
	net = {
		HOLD = "hold",
		intercept = function(definition)
			intercept = intercept or definition
			intercepts[#intercepts + 1] = definition
		end,
		request = function(_, fn)
			network = fn
		end,
		release = function(id)
			released = id
		end,
	},
	ui = {
		image_rewrite = function(_, fn)
			rewrite = fn
		end,
	},
}
dofile(arg[1])
local request = { id = "7", path = "/api/v9/users/42/profile" }
assert(intercept.request(request) == "hold")
network({
	json = { Vencord = { "Contributor" }, Equicord = { { name = "Donor", badge = "https://example.com/d.png" } } },
})
assert(released == "7")
local result = intercept.response(request, { status = 200, body = "{}" })
assert(#result.body.badges == 2)
local icon = result.body.badges[1].icon
local url = rewrite("https://cdn.discordapp.com/badge-icons/" .. icon .. ".png")
assert(url and (url:find("example.com", 1, true) or url:find("clientmodbadges", 1, true)))
assert(
	result.body.badges[1].description == "Vencord: Contributor"
		or result.body.badges[2].description == "Vencord: Contributor"
)
assert(intercept.request(request) == nil)
local catalog = intercepts[2].response({ path = "/api/v9/users/@me/badges" }, { status = 200, body = "catalog" })
assert(#catalog.body.badges == 3 and catalog.body.badges[2].owned and catalog.body.badges[2].badge_id > 22)
print("cross-mod-badges ok")
