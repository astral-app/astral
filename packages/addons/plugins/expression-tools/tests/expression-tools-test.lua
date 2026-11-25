local inserts, patches, presses = {}, {}, {}
local copied, toasts, opened, offered, sheet_spec, sheet_handler = nil, {}, nil, nil, nil, nil
local timers = {}
local function run()
	local pending = timers
	timers = {}
	for _, fn in ipairs(pending) do
		fn()
	end
end
astral = {
	i18n = {
		t = function(key)
			return key
		end,
	},
	time = {
		after = function(_, fn)
			timers[#timers + 1] = fn
		end,
	},
	base64 = {
		decode = function(text)
			return text == "UE5H" and "PNG" or nil
		end,
	},
	net = {
		request = function(options, callback)
			assert(options.url:find("cdn.discordapp.com/emojis/42.png", 1, true))
			callback({ status = 200, body = "", body_base64 = "UE5H" })
		end,
	},
	media = {
		offer_pick = function(bytes, name)
			offered = { bytes = bytes, name = name }
			return true
		end,
	},
	discord = {
		emoji = function(id)
			return id == "42" and { id = "42", name = "wave", animated = false } or nil
		end,
		me = function()
			return "1"
		end,
		guilds = function()
			return { { id = "7", name = "Zed" }, { id = "8", name = "Alpha" }, { id = "9", name = "None" } }
		end,
		permissions = function(guild)
			return ({ ["7"] = 1073741824, ["8"] = 8, ["9"] = 0 })[guild]
		end,
	},
	ui = {
		insert = function(selector, build, press)
			inserts[selector] = { build = build, press = press }
		end,
		patch = function(selector, fn)
			patches[selector] = fn
		end,
		copy = function(text)
			copied = text
			return true
		end,
		toast = function(_, text)
			toasts[#toasts + 1] = text
		end,
		sheet = function(spec, handler)
			sheet_spec, sheet_handler = spec, handler
			return { close = function() end }
		end,
		dismiss = function()
			return true
		end,
		open = function(url)
			opened = url
			return true
		end,
		press = function(id)
			presses[#presses + 1] = id
			return true
		end,
	},
}
dofile(arg[1])

local image, menu = inserts["image:https://cdn.discordapp.com/emojis/"], inserts["Copy Emoji Image Link^"]
assert(menu.build({}) == nil)
assert(patches["Copy Emoji Image Link"]() == nil)
image.build({ key = "42.webp?size=48&animated=true" })
assert(menu.build({}) == nil)
image.build({ key = "42.webp?size=160&animated=true" })
local rows = menu.build({})
assert(#rows == 4 and rows[2].action == "markup" and rows[4].action == "clone")
assert(patches["Copy Emoji Image Link"]().radius == 0)

menu.press({}, "markup")
assert(copied == "<:wave:42>" and toasts[#toasts] == "MARKUP_COPIED")

menu.press({}, "clone")
local names = {}
for _, item in ipairs(sheet_spec.items) do
	if item.type == "row" then
		names[#names + 1] = item.items[1].text
	end
end
assert(#names == 2 and names[1] == "Alpha" and names[2] == "Zed")
sheet_handler("guild:1")
assert(offered.bytes == "PNG" and offered.name == "wave.png")
run()
assert(opened == "discord://-/guilds/8/settings/emoji")
run()
assert(presses[1] == "Upload Emoji")
print("expression-tools ok")
