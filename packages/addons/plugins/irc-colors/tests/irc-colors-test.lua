local previous_path = package.path
package.path = arg[1]:gsub("main.lua$", "?.lua;") .. previous_path

local xxhash64 = require("lib.xxhash64")
assert(xxhash64.hash("224617799434108928shuffle-seed-that-is-long-enough-for-stripes") == -6643768567790270702)
assert(xxhash64.unsigned_modulo(-1, 360) == 15)

local values = { lightness = 70, saturation = 100, seed = "", onlyWithoutColor = false, onlyInDms = false }
local colorize

astral = {
	PatchNodeType = { Text = "text" },
	i18n = {
		t = function(key)
			return key
		end,
	},
	settings = {
		values = values,
		number = function() end,
		text = function() end,
		switch = function() end,
	},
	discord = {
		guild = function()
			return { roles = { r = { colors = { primary_color = 0x112233 } } } }
		end,
		member = function()
			return { roles = { "r" } }
		end,
		channel_guild = function(channel)
			return channel == "gc" and "g" or nil
		end,
	},
	ui = {
		patch = function(_, callback)
			colorize = function(node)
				local patch = callback(node)
				return patch and patch.text_color
			end
		end,
	},
}

dofile(arg[1])

local function hue(color)
	local r, g, b = (color >> 16) & 0xFF, (color >> 8) & 0xFF, color & 0xFF
	local high, low = math.max(r, g, b), math.min(r, g, b)
	local delta = high - low
	local degrees
	if high == r then
		degrees = 60 * (((g - b) / delta) % 6)
	elseif high == g then
		degrees = 60 * ((b - r) / delta + 2)
	else
		degrees = 60 * ((r - g) / delta + 4)
	end
	return math.floor(degrees + 0.5)
end

local color = colorize({ author_id = "224617799434108928" })
assert(color >> 24 == 0xFF)
assert(math.abs(hue(color) - 109) <= 1)
assert(colorize({}) == nil)

values.onlyInDms = true
assert(colorize({ author_id = "1", channel_id = "gc" }) == nil)
assert(colorize({ author_id = "1" }) ~= nil)
values.onlyInDms = false
values.onlyWithoutColor = true
assert(colorize({ author_id = "1", channel_id = "gc" }) == nil)

values.seed = "shuffle"
assert(colorize({ author_id = "224617799434108928" }) ~= color)
print("irc-colors ok")
