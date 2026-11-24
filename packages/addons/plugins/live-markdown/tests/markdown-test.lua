local folder = arg[1]:match("(.*/)") or "./"
package.path = folder .. "?.lua;" .. package.path

local formatter
astral = { ui = {
	composer = function(callback)
		formatter = callback
	end,
} }
dofile(arg[1])

local function styled(text)
	local result = {}
	local runs = formatter(text)
	table.sort(runs, function(first, second)
		return first.start < second.start
	end)
	for _, run in ipairs(runs) do
		if run.style ~= "marker" then
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

print("live-markdown ok")
