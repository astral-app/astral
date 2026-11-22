local P = astral.discord.permission
local t = astral.i18n.t

local TAGS = {
	{
		key = "WEBHOOK",
		text = t("TAG_WEBHOOK"),
		color = 0xFF5865F2,
		title = t("TAG_WEBHOOK"),
		subtitle = t("TAG_WEBHOOK_SUBTITLE"),
	},
	{ key = "OWNER", text = "Owner", color = 0xFFF0B232, title = "Owner", subtitle = "Owns the server" },
	{
		key = "ADMINISTRATOR",
		text = t("TAG_ADMINISTRATOR"),
		color = 0xFFDA373C,
		title = t("TAG_ADMINISTRATOR"),
		subtitle = t("TAG_ADMINISTRATOR_SUBTITLE"),
		permissions = P.ADMINISTRATOR,
	},
	{
		key = "MODERATOR_STAFF",
		text = t("TAG_MODERATOR_STAFF"),
		color = 0xFF248046,
		title = t("TAG_MODERATOR_STAFF"),
		subtitle = t("TAG_MODERATOR_STAFF_SUBTITLE"),
		permissions = P.MANAGE_GUILD | P.MANAGE_CHANNELS | P.MANAGE_ROLES | P.MANAGE_WEBHOOKS,
	},
	{
		key = "MODERATOR",
		text = t("TAG_MODERATOR"),
		color = 0xFF4E7FFF,
		title = t("TAG_MODERATOR"),
		subtitle = t("TAG_MODERATOR_SUBTITLE"),
		permissions = P.MANAGE_MESSAGES | P.KICK_MEMBERS | P.BAN_MEMBERS,
	},
	{
		key = "VOICE_MODERATOR",
		text = t("TAG_VOICE_MODERATOR"),
		color = 0xFF059669,
		title = t("TAG_VOICE_MODERATOR"),
		subtitle = t("TAG_VOICE_MODERATOR_SUBTITLE"),
		permissions = P.MOVE_MEMBERS | P.MUTE_MEMBERS | P.DEAFEN_MEMBERS,
	},
	{
		key = "CHAT_MODERATOR",
		text = t("TAG_CHAT_MODERATOR"),
		color = 0xFFC27C0E,
		title = t("TAG_CHAT_MODERATOR"),
		subtitle = t("TAG_CHAT_MODERATOR_SUBTITLE"),
		permissions = P.MODERATE_MEMBERS,
	},
}

local APPEARANCE = {
	{
		key = "coloredTags",
		title = t("COLORED_TAGS"),
		subtitle = t("COLORED_TAGS_SUBTITLE"),
		default = true,
	},
	{
		key = "useRoleColor",
		title = t("USE_ROLE_COLOR"),
		subtitle = t("USE_ROLE_COLOR_SUBTITLE"),
		default = true,
	},
	{
		key = "useEnhancedRoleColors",
		title = t("USE_ENHANCED_ROLE_COLORS"),
		subtitle = t("USE_ENHANCED_ROLE_COLORS_SUBTITLE"),
		default = true,
	},
	{
		key = "useOpTagStyle",
		title = t("USE_OP_TAG_STYLE"),
		subtitle = t("USE_OP_TAG_STYLE_SUBTITLE"),
		default = false,
	},
}

local ROLE_NAME = {
	{
		key = "showRoleName",
		title = t("SHOW_ROLE_NAME"),
		subtitle = t("SHOW_ROLE_NAME_SUBTITLE"),
		default = false,
	},
	{
		key = "roleNameOnBots",
		title = t("ROLE_NAME_ON_BOTS"),
		subtitle = t("ROLE_NAME_ON_BOTS_SUBTITLE"),
		default = false,
	},
}

local DEFAULT_TAG_COLOR = 0xFF5865F2
local settings = {}

for _, tag in ipairs(TAGS) do
	settings[tag.key] =
		astral.settings.switch(tag.key, tag.title, true, { subtitle = tag.subtitle, section = t("TAGS") })
end
for _, option in ipairs(APPEARANCE) do
	settings[option.key] = astral.settings.switch(
		option.key,
		option.title,
		option.default,
		{ subtitle = option.subtitle, section = t("APPEARANCE") }
	)
end
for _, option in ipairs(ROLE_NAME) do
	settings[option.key] = astral.settings.switch(
		option.key,
		option.title,
		option.default,
		{ subtitle = option.subtitle, section = t("ROLE_NAME") }
	)
end

function astral.on_setting(key, value)
	settings[key] = value
end

local function opaque(color)
	return 0xFF000000 | (color & 0xFFFFFF)
end

local function roleStops(role, includeEnhanced)
	local colors = role.colors or {}
	local primary = colors.primary_color
	if not primary or primary == 0 then
		return {}
	end
	local stops = { opaque(primary) }
	if includeEnhanced then
		for _, name in ipairs({ "secondary_color", "tertiary_color" }) do
			local value = colors[name]
			if value and value ~= 0 then
				local stop = opaque(value)
				local seen = false
				for _, existing in ipairs(stops) do
					seen = seen or existing == stop
				end
				if not seen then
					stops[#stops + 1] = stop
				end
			end
		end
	end
	return stops
end

local function topColouredRole(guild, member)
	local best, bestPosition = nil, -1
	for _, roleId in ipairs(member.roles or {}) do
		local role = guild.roles[roleId]
		if role and role.colors and (role.colors.primary_color or 0) ~= 0 and (role.position or 0) > bestPosition then
			best, bestPosition = role, role.position or 0
		end
	end
	return best
end

local function memberRoleStops(guild, member, includeEnhanced)
	local best = topColouredRole(guild, member)
	return best and roleStops(best, includeEnhanced) or {}
end

local function luminance(color)
	local total = 0
	for index, weight in ipairs({ 0.2126, 0.7152, 0.0722 }) do
		local channel = ((color >> (24 - index * 8)) & 0xFF) / 255
		channel = channel <= 0.04045 and channel / 12.92 or ((channel + 0.055) / 1.055) ^ 2.4
		total = total + channel * weight
	end
	return total
end

local function contrastingText(backgrounds)
	local white, black = math.huge, math.huge
	for _, color in ipairs(backgrounds) do
		local value = luminance(color)
		white = math.min(white, 1.05 / (value + 0.05))
		black = math.min(black, (value + 0.05) / 0.05)
	end
	return black > white and 0xFF000000 or 0xFFFFFFFF
end

local function hasPermission(permissions, wanted)
	if not permissions then
		return false
	end
	return permissions & wanted ~= 0
end

local function resolveTag(node, guild, message)
	local isBot = message and message.author and message.author.bot
	local permissions = nil
	for _, tag in ipairs(TAGS) do
		if settings[tag.key] then
			if tag.key == "WEBHOOK" then
				if message and message.webhook_id and isBot then
					return tag
				end
			elseif tag.key == "OWNER" then
				if guild and guild.owner_id == node.user_id then
					return tag
				end
			elseif guild and not isBot then
				permissions = permissions or astral.discord.permissions(guild.id, node.user_id, node.channel_id)
				if hasPermission(permissions, tag.permissions) then
					return tag
				end
			end
		end
	end
	return nil
end

local function tagColors(tag, guild, node)
	if settings.useRoleColor and guild then
		local member = astral.discord.member(guild.id, node.user_id)
		if member then
			local stops = memberRoleStops(guild, member, settings.useEnhancedRoleColors)
			if #stops > 0 then
				return stops
			end
		end
	end
	return settings.coloredTags and { tag.color } or {}
end

local function pillFor(text, stops, shape)
	local background = stops[1] or DEFAULT_TAG_COLOR
	local pill = {
		text = text,
		color = background,
		text_color = contrastingText(#stops > 0 and stops or { background }),
		shape = shape,
	}
	if #stops >= 2 then
		pill.colors = stops
	end
	return pill
end

local function roleNamePill(guild, node, isBot)
	if not settings.showRoleName or not guild or (isBot and not settings.roleNameOnBots) then
		return nil
	end
	local member = astral.discord.member(guild.id, node.user_id)
	local role = member and topColouredRole(guild, member)
	if not role then
		return nil
	end
	return pillFor(role.name, roleStops(role, settings.useEnhancedRoleColors), "pill")
end

astral.ui.decorate("message.author", function(node)
	if not node.user_id then
		return nil
	end
	local message = node.channel_id and astral.discord.message(node.channel_id, node.message_id) or nil
	local guild = node.guild_id and astral.discord.guild(node.guild_id) or nil
	local isBot = message and message.author and message.author.bot
	local pills = {}
	local tag = resolveTag(node, guild, message)
	if tag then
		pills[#pills + 1] = pillFor(tag.text, tagColors(tag, guild, node), settings.useOpTagStyle and "op" or "pill")
	end
	local roleName = roleNamePill(guild, node, isBot)
	if roleName then
		pills[#pills + 1] = roleName
	end
	return #pills > 0 and pills or nil
end)

astral.log("ready")
