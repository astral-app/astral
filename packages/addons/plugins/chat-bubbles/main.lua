local t = astral.i18n.t

local AUTOMATIC = 0x33000000
local MENTION = "#F0B232"

local colors = astral.settings.section(t("COLORS"))
colors:color("color", t("BUBBLE_COLOR"), "", t("BUBBLE_COLOR_FALLBACK"))
colors:color("ownColor", t("OWN_COLOR"), "", t("OWN_COLOR_FALLBACK"))
colors:number("opacity", t("OPACITY"), { min = 5, max = 100, step = 5, default = 100 })

local mentions = astral.settings.section(t("MENTIONS"))
mentions:color("mentionColor", t("MENTION_COLOR"), MENTION, t("MENTION_COLOR_FALLBACK"))
mentions:number("mentionTint", t("MENTION_TINT"), { min = 0, max = 60, step = 2, default = 12 })

local shape = astral.settings.section(t("SHAPE"))
shape:number("radius", t("CORNER_RADIUS"), { min = 0, max = 32, step = 1, default = 16 })

local spacing = astral.settings.section(t("SPACING_SECTION"))
spacing:number("horizontalPadding", t("HORIZONTAL_PADDING"), { min = 4, max = 24, step = 1, default = 14 })
spacing:number("verticalPadding", t("VERTICAL_PADDING"), { min = 2, max = 20, step = 1, default = 10 })

local layout = astral.settings.section(t("NAME_AVATAR"))
layout:choice("name", t("NAME_INSIDE"), {
	astral.settings.option(astral.NamePlacement.Inside, t("NAME_INSIDE"), t("NAME_INSIDE_SUBTITLE")),
	astral.settings.option(astral.NamePlacement.Above, t("NAME_ABOVE"), t("NAME_ABOVE_SUBTITLE")),
}, astral.NamePlacement.Inside)
layout:choice("time", t("TIME"), {
	astral.settings.option(astral.TimePlacement.Header, t("TIME_HEADER"), t("TIME_HEADER_SUBTITLE")),
	astral.settings.option(astral.TimePlacement.Inline, t("TIME_INLINE"), t("TIME_INLINE_SUBTITLE")),
	astral.settings.option(astral.TimePlacement.Hidden, t("TIME_HIDDEN"), t("TIME_HIDDEN_SUBTITLE")),
}, astral.TimePlacement.Header)
layout:choice("reply", t("REPLY"), {
	astral.settings.option(astral.ReplyPlacement.Inside, t("REPLY_QUOTE")),
	astral.settings.option(astral.ReplyPlacement.Above, t("REPLY_ICON")),
	astral.settings.option(astral.ReplyPlacement.Bubble, t("REPLY_BUBBLE")),
}, astral.ReplyPlacement.Inside, t("REPLY_SUBTITLE"))

local function picked(value)
	return value ~= nil and value ~= ""
end

local function with_opacity(color, values)
	local alpha = math.floor((values.opacity or 100) * 255 / 100 + 0.5)
	return astral.Color.from(color):with_alpha(alpha)
end

local function mentions_me(message)
	if type(message) ~= "table" then
		return false
	end
	if message.mention_everyone == true then
		return true
	end
	local me = astral.discord.me()
	for _, user in ipairs(type(message.mentions) == "table" and message.mentions or {}) do
		if type(user) == "table" and user.id == me then
			return true
		end
	end
	return false
end

local function tinted(base, values)
	local tint = (values.mentionTint or 12) / 100
	local mr, mg, mb = astral.Color.from(values.mentionColor or MENTION):channels()
	local br, bg, bb, ba = astral.Color.from(base):channels()
	local function mix(from, to)
		return math.floor(from + (to - from) * tint + 0.5)
	end
	return astral.Color.rgb(mix(br, mr), mix(bg, mg), mix(bb, mb), math.max(ba, math.floor(255 * tint + 0.5)))
end

local function bubble_color(node, values)
	local own = picked(values.ownColor) and node.author_id ~= nil and node.author_id == astral.discord.me()
	local base = own and with_opacity(values.ownColor, values)
		or (picked(values.color) and with_opacity(values.color, values) or AUTOMATIC)
	local message = node.message_id and node.channel_id and astral.discord.message(node.channel_id, node.message_id)
	if not own and mentions_me(message) then
		return tinted(base, values)
	end
	return base
end

astral.ui.message_background(function(node)
	local values = astral.settings.values
	return {
		color = bubble_color(node, values),
		radius = values.radius or 16,
		bubble = true,
		reply = values.reply,
		name = values.name,
		time = values.time,
		padding_x = values.horizontalPadding or 14,
		padding_y = values.verticalPadding or 10,
	}
end)
