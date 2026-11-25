local LOOKUP = "https://pronoundb.org/api/v2/lookup?platform=discord&ids="
local BATCH = 50
local NAMES = {
	he = { "he", "him" },
	she = { "she", "her" },
	it = { "it", "its" },
	they = { "they", "them" },
	any = { "any pronouns" },
	ask = { "ask me my pronouns" },
	avoid = { "avoid pronouns, use my name" },
	other = { "other pronouns" },
}

local settings = astral.settings.section("Pronouns")
settings:choice("format", "Format", {
	astral.settings.option("lowercase", "he/him"),
	astral.settings.option("capitalized", "He/Him"),
}, "lowercase")
settings:choice("style", "Style", {
	astral.settings.option("text", "Text"),
	astral.settings.option("pill", "Pill"),
}, "text")

local pronouns = {}
local pending, in_flight = {}, false

local function format_sets(sets)
	local english = type(sets) == "table" and sets.en
	if type(english) ~= "table" or #english == 0 then
		return false
	end
	local parts = {}
	if #english == 1 then
		parts = NAMES[english[1]] or { english[1] }
	else
		for _, set in ipairs(english) do
			parts[#parts + 1] = (NAMES[set] or { set })[1]
		end
	end
	local text = table.concat(parts, "/")
	if astral.settings.get("format", "lowercase") == "capitalized" then
		text = text:gsub("(%a)(%a*)", function(first, rest)
			return first:upper() .. rest
		end)
	end
	return text
end

local function flush()
	if in_flight then
		return
	end
	local ids = {}
	for id in pairs(pending) do
		ids[#ids + 1] = id
		if #ids == BATCH then
			break
		end
	end
	if #ids == 0 then
		return
	end
	for _, id in ipairs(ids) do
		pending[id] = nil
	end
	in_flight = true
	astral.net.request({ url = LOOKUP .. table.concat(ids, ",") }, function(response)
		in_flight = false
		local body = response and response.json or {}
		for _, id in ipairs(ids) do
			pronouns[id] = format_sets(type(body[id]) == "table" and body[id].sets)
		end
		astral.ui.refresh()
		flush()
	end)
end

astral.ui.decorate(astral.Target.MessageAuthor, function(node)
	local id = node.user_id
	if not id then
		return nil
	end
	local known = pronouns[id]
	if known == nil then
		pending[id] = true
		pronouns[id] = false
		astral.time.after(50, flush)
		return nil
	end
	if not known then
		return nil
	end
	if astral.settings.get("style", "text") == "pill" then
		return astral.Pill(known)
	end
	return astral.Pill(known):gradient({})
end)
