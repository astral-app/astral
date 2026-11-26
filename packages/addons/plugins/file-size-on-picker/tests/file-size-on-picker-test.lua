local build
local media = {
	["picked://p1"] = { size = 2812524, duration = 0, type = astral.PickedMediaType.Image },
	["picked://p2"] = { size = 132680, duration = 4.5, type = astral.PickedMediaType.Video },
	["picked://p3"] = { size = 0, duration = 0, type = astral.PickedMediaType.Image },
	["picked://p4"] = { size = 512, duration = 0, type = astral.PickedMediaType.Image },
}
astral = {
	i18n = {
		t = function(key)
			return key == "BYTES" and "Bytes" or key
		end,
	},
	media = {
		picked = function(uri)
			return media[uri]
		end,
	},
	ui = {
		insert = function(selector, fn)
			assert(selector.id == "media-tile-" and selector.type == astral.PatchNodeType.Stack)
			build = fn
		end,
	},
}
dofile(arg[1])

local function label(key)
	local node = build({ id = "media-tile-" .. key, key = key, type = astral.PatchNodeType.Stack })
	return node and node.type == astral.InsertNodeType.View and node.children[1].text
end

assert(label("1") == "2.68 MB", label("1"))
assert(label("2") == "129.57 KB", label("2"))
assert(label("3") == "0 Bytes", label("3"))
assert(label("4") == "512 Bytes", label("4"))
assert(label("9") == nil)
assert(label("camera") == nil)
