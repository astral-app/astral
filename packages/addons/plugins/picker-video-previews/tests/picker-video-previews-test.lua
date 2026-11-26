local build
local refreshed = 0
local media = {
	["picked://p1"] = { type = astral.PickedMediaType.Image, duration = 0, size = 10 },
	["picked://p2"] = { type = astral.PickedMediaType.Video, duration = 4.5, size = 10 },
}
local urls = { ["picked://p2"] = "https://media.discordapp.net/astral-picked/token.mp4" }
local values = { sound = false }
local changes = {}
astral = {
	i18n = {
		t = function(key)
			return key
		end,
	},
	settings = {
		values = values,
		section = function()
			return { switch = function() end }
		end,
		on_change = function(key, fn)
			changes[key] = fn
		end,
	},
	media = {
		picked = function(uri)
			return media[uri]
		end,
		picked_url = function(uri)
			return urls[uri]
		end,
	},
	ui = {
		insert = function(selector, fn)
			assert(selector == "image:picked://")
			build = fn
		end,
		refresh = function()
			refreshed = refreshed + 1
		end,
	},
}
dofile(arg[1])

local player = build({ id = "image:picked://p2", key = "p2", type = astral.PatchNodeType.Image })
assert(player.type == astral.InsertNodeType.Player and player.uri == urls["picked://p2"] and player.muted == true)
assert(player.fill and player.fill_height)
assert(build({ id = "image:picked://p1", key = "p1", type = astral.PatchNodeType.Image }) == nil)
assert(build({ id = "image:picked://p3", key = "p3", type = astral.PatchNodeType.Image }) == nil)
assert(
	build({ id = "image:picked://p2", key = "p2", type = astral.PatchNodeType.Image, parent_id = "media-tile-2" })
		== nil
)
values.sound = true
changes.sound()
assert(refreshed == 1)
assert(build({ id = "image:picked://p2", key = "p2", type = astral.PatchNodeType.Image }).muted == false)
