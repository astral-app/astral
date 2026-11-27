local folder = arg[1]:match("(.*/)") or "./"
package.path = folder .. "?.lua;" .. package.path

local formatter
astral = {
	ui = {
		composer = function(callback)
			formatter = callback
		end,
	},
	time = {
		now_ms = function()
			return 0
		end,
	},
	discord = {
		guilds = function()
			return { { id = "1" }, { id = "2" } }
		end,
		guild = function(id)
			if id == "1" then
				return { id = "1", emojis = { { id = "10", name = "GitHub", animated = false } } }
			end
			return { id = "2", emojis = { { id = "20", name = "wave", animated = true } } }
		end,
	},
}
dofile(arg[1])

local function styled(text)
	local result = {}
	local runs = formatter(text, { guild_id = "1" })
	table.sort(runs, function(first, second)
		return first.start < second.start
	end)
	for _, run in ipairs(runs) do
		if run.style ~= "marker" and run.style ~= "image" then
			result[#result + 1] = { run.style, text:sub(run.start, run.start + run.length - 1) }
		end
	end
	return result
end

local function expect(actual, expected)
	assert(#actual == #expected)
	for index, item in ipairs(expected) do
		assert(actual[index][1] == item[1], "unexpected style at " .. index)
		assert(actual[index][2] == item[2], "unexpected text at " .. index)
	end
end

expect(styled("a **bold** and *it* and ~~gone~~"), {
	{ "bold", "bold" },
	{ "italic", "it" },
	{ "strike", "gone" },
})
expect(styled("🙂 **é**"), { { "bold", "é" } })
expect(styled("__under__ ||secret||"), {
	{ "underline", "under" },
	{ "spoiler", "secret" },
})
expect(styled("see `**not bold**` here"), { { "code", "**not bold**" } })
expect(styled("```\n*a*\n```"), { { "code", "\n*a*\n" } })
expect(styled("# Title\n-# small\n> said"), {
	{ "heading1", "Title" },
	{ "subtext", "small" },
	{ "quote", "said" },
})
expect(styled("**open and * spaced *"), {})
expect(styled("2 * 3 * 4"), {})
expect(styled("nice :blob_cat_happy: and :party_parrot:"), {})
expect(styled("<:blob_cat:123> <a:wave_hi:456>"), {})
expect(styled("call snake_case_name or __init__"), { { "underline", "init" } })
expect(styled("an _italic_ word, a__b__c"), { { "italic", "italic" } })
assert(formatter(string.rep("x", 8193))[1] == nil)

local function images(text)
	local found = {}
	for _, run in ipairs(formatter(text, { guild_id = "1" })) do
		if run.style == "image" then
			found[#found + 1] = { text:sub(run.start, run.start + run.length - 1), run.image }
		end
	end
	return found
end

local picked = images("hi :GitHub: and :wave: :nope: <a:party:30>")
assert(#picked == 3)
assert(picked[1][1] == ":GitHub:" and picked[1][2] == "https://cdn.discordapp.com/emojis/10.png?size=96")
assert(picked[2][1] == ":wave:" and picked[2][2] == "https://cdn.discordapp.com/emojis/20.gif?size=96")
assert(picked[3][1] == "<a:party:30>" and picked[3][2] == "https://cdn.discordapp.com/emojis/30.gif?size=96")
assert(#images("time 10:30:45") == 0)
print("live-markdown ok")
