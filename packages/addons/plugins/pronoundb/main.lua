local t = astral.i18n.t
local Pill, Shape = astral.Pill, astral.Shape

local LOOKUP = "https://pronoundb.org/api/v2/lookup?platform=discord&ids="
local BATCH = 50
local PROFILE_SPACING_MS = 300
local SETS = {
	he = { "he", "him" },
	she = { "she", "her" },
	it = { "it", "its" },
	they = { "they", "them" },
}
local PHRASES = {
	any = "PRONOUN_ANY",
	ask = "PRONOUN_ASK",
	avoid = "PRONOUN_AVOID",
	other = "PRONOUN_OTHER",
}

local settings = astral.settings.section(t("PRONOUNS"))
settings:choice("format", t("PRONOUNS"), {
	astral.settings.option("lowercase", t("FORMAT_LOWERCASE")),
	astral.settings.option("capitalized", t("FORMAT_CAPITALIZED")),
}, "lowercase")
settings:choice("priority", t("SOURCE"), {
	astral.settings.option("pronoundb", t("SOURCE_PRONOUNDB"), t("SOURCE_PRONOUNDB_SUBTITLE")),
	astral.settings.option("discord", t("SOURCE_DISCORD"), t("SOURCE_DISCORD_SUBTITLE")),
}, "pronoundb")
settings:choice("style", t("STYLE"), {
	astral.settings.option("text", t("STYLE_TEXT")),
	astral.settings.option("pill", t("STYLE_PILL")),
	astral.settings.option("custom", t("STYLE_CUSTOM")),
}, "text")
settings:color("pillColor", t("PILL_COLOR"), "#5865F2")
settings:switch("showSelf", t("SHOW_FOR_YOURSELF"), true)

local from_pronoundb, from_discord = {}, {}
local pending, in_flight = {}, false
local profile_queue, profile_busy = {}, false

local function capitalize(text)
	return (text:gsub("(%a)(%a*)", function(first, rest)
		return first:upper() .. rest
	end))
end

local function format_sets(sets)
	local english = type(sets) == "table" and sets.en
	if type(english) ~= "table" or #english == 0 then
		return false
	end
	local phrase = #english == 1 and PHRASES[english[1]]
	if phrase then
		return t(phrase)
	end
	local parts = {}
	if #english == 1 then
		parts = SETS[english[1]] or { english[1] }
	else
		for _, set in ipairs(english) do
			parts[#parts + 1] = (SETS[set] or { set })[1]
		end
	end
	return table.concat(parts, "/")
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
			from_pronoundb[id] = format_sets(type(body[id]) == "table" and body[id].sets)
		end
		astral.ui.refresh()
		flush()
	end)
end

local function pump_profiles()
	if profile_busy or #profile_queue == 0 then
		return
	end
	local id = table.remove(profile_queue, 1)
	profile_busy = true
	local function done(pronouns)
		from_discord[id] = type(pronouns) == "string" and pronouns ~= "" and pronouns or false
		if from_discord[id] then
			astral.ui.refresh()
		end
		astral.time.after(PROFILE_SPACING_MS, function()
			profile_busy = false
			pump_profiles()
		end)
	end
	local ok = pcall(function()
		astral.discord.request({ path = "/users/" .. id .. "/profile" }, function(response)
			local ok, body = pcall(astral.json.decode, response and response.ok and response.body or "")
			local profile = ok and type(body) == "table" and type(body.user_profile) == "table" and body.user_profile
				or {}
			done(profile.pronouns)
		end)
	end)
	if not ok then
		done(nil)
	end
end

local function lookup(id)
	if from_pronoundb[id] == nil then
		from_pronoundb[id] = false
		pending[id] = true
		astral.time.after(50, flush)
	end
	if from_discord[id] == nil then
		from_discord[id] = false
		profile_queue[#profile_queue + 1] = id
		pump_profiles()
	end
	local preferred, fallback = from_pronoundb[id], from_discord[id]
	if astral.settings.get("priority", "pronoundb") == "discord" then
		preferred, fallback = fallback, preferred
	end
	return preferred or fallback or nil
end

local function is_self(id)
	return astral.discord.me() == id
end

astral.ui.decorate(astral.Target.MessageAuthor, function(node)
	local id = node.user_id
	if not id or (not astral.settings.get("showSelf", true) and is_self(id)) then
		return nil
	end
	local pronouns = lookup(id)
	if not pronouns then
		return nil
	end
	if astral.settings.get("format", "lowercase") == "capitalized" then
		pronouns = capitalize(pronouns)
	end
	local style = astral.settings.get("style", "text")
	if style == "text" then
		return Pill("• " .. pronouns):shaped(Shape.Text)
	end
	if style == "custom" then
		return Pill(pronouns):color(tostring(astral.settings.get("pillColor", "#5865F2")))
	end
	return Pill(pronouns)
end)
