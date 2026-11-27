local t = astral.i18n.t
local TONE_TAGS = {
	"j",
	"hj",
	"s",
	"sarc",
	"srs",
	"nsrs",
	"lh",
	"g",
	"gen",
	"genq",
	"gq",
	"rh",
	"rhet",
	"rq",
	"pos",
	"pc",
	"neg",
	"nc",
	"neu",
	"nm",
	"nbh",
	"nay",
	"ay",
	"nsx",
	"sx",
	"t",
	"ref",
	"lyr",
	"ly",
	"c",
	"m",
	"li",
	"hyp",
	"f",
	"th",
	"cb",
	"ij",
	"ot",
	"nf",
	"nsb",
	"p",
	"r",
	"q",
	"u",
	"x",
	"info",
	"sbtw",
}
local TONES = {}
for _, tag in ipairs(TONE_TAGS) do
	TONES[tag] = t("TONE_" .. tag:upper())
end
local PILL = 0x33949BA4

local settings = astral.settings.section(t("TONE_TAGS"))
settings:switch("explain", t("EXPLAIN"), true, t("EXPLAIN_SUBTITLE"))
settings:text("custom", t("CUSTOM_TAGS"), "", {
	subtitle = t("CUSTOM_TAGS_SUBTITLE"),
	max_length = 4000,
})

local function custom_tones()
	local tones = {}
	for line in tostring(astral.settings.get("custom", "") or ""):gmatch("[^\n]+") do
		local tag, meaning = line:match("^%s*/?(%a+)%s*=%s*(.-)%s*$")
		if tag and #tag <= 6 and meaning ~= "" then
			tones[tag:lower()] = meaning
		end
	end
	return tones
end

local function meaning_of(tag, custom)
	local key = tag:lower()
	return custom[key] or TONES[key]
end

local function pill(tag, meaning, base)
	local text = astral.settings.get("explain", true) and (tag .. " " .. meaning:lower()) or tag
	return { text = text, bg = PILL, bg_radius = 4, color = base.color, weight = 600, link = base.link }
end

local function split_span(span, custom, out)
	local text = span.text
	if type(text) ~= "string" or span._raw or not text:find("/", 1, true) then
		out[#out + 1] = span
		return false
	end
	local changed, emitted, search = false, 1, 1
	while true do
		local start, finish, tag = text:find("%f[^%s%z]/(%a+)", search)
		if not start then
			break
		end
		local after = text:sub(finish + 1, finish + 1)
		local meaning = #tag <= 6 and (after == "" or after:match("[%s%.,!%?;:%)%]]")) and meaning_of(tag, custom)
		if meaning then
			if start > emitted then
				out[#out + 1] =
					{ text = text:sub(emitted, start - 1), color = span.color, weight = span.weight, link = span.link }
			end
			out[#out + 1] = pill("/" .. tag, meaning, span)
			changed = true
			emitted = finish + 1
		end
		search = finish + 1
	end
	if not changed then
		out[#out + 1] = span
		return false
	end
	if emitted <= #text then
		out[#out + 1] = { text = text:sub(emitted), color = span.color, weight = span.weight, link = span.link }
	end
	return true
end

astral.ui.text_transform(function(spans)
	local custom = custom_tones()
	local out, changed = {}, false
	for _, span in ipairs(spans) do
		changed = split_span(span, custom, out) or changed
	end
	return changed and out or nil
end)
