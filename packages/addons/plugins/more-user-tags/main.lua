local Color, Permission, Pill, Shape = astral.Color, astral.Permission, astral.Pill, astral.Shape
local permissions = astral.permissions
local settings = astral.settings.values
local t = astral.i18n.t

local Tag = astral.class("Tag")

function Tag:init(key, color, applies)
	self.key = key
	self.text = t("TAG_" .. key)
	self.subtitle = t("TAG_" .. key .. "_SUBTITLE")
	self.color = Color.from(color)
	self.applies = applies
end

local function with_permissions(...)
	local wanted = permissions.combine(...)
	return function(node)
		return not node:is_bot() and permissions.any(node:permissions(), wanted)
	end
end

local TAGS = {
	Tag("WEBHOOK", "#5865F2", function(node)
		local message = node:message()
		return message ~= nil and message.webhook_id ~= nil and node:is_bot()
	end),
	Tag("OWNER", "#F0B232", function(node)
		local guild = node:guild()
		return guild ~= nil and guild.owner_id == node.user_id
	end),
	Tag("ADMINISTRATOR", "#DA373C", with_permissions(Permission.ADMINISTRATOR)),
	Tag(
		"MODERATOR_STAFF",
		"#248046",
		with_permissions(
			Permission.MANAGE_GUILD,
			Permission.MANAGE_CHANNELS,
			Permission.MANAGE_ROLES,
			Permission.MANAGE_WEBHOOKS
		)
	),
	Tag(
		"MODERATOR",
		"#4E7FFF",
		with_permissions(Permission.MANAGE_MESSAGES, Permission.KICK_MEMBERS, Permission.BAN_MEMBERS)
	),
	Tag(
		"VOICE_MODERATOR",
		"#059669",
		with_permissions(Permission.MOVE_MEMBERS, Permission.MUTE_MEMBERS, Permission.DEAFEN_MEMBERS)
	),
	Tag("CHAT_MODERATOR", "#C27C0E", with_permissions(Permission.MODERATE_MEMBERS)),
}

local tags = astral.settings.section(t("TAGS"))
for _, tag in ipairs(TAGS) do
	tags:switch(tag.key, tag.text, true, tag.subtitle)
end

local appearance = astral.settings.section(t("APPEARANCE"))
appearance:switch("coloredTags", t("COLORED_TAGS"), true, t("COLORED_TAGS_SUBTITLE"))
appearance:switch("useRoleColor", t("USE_ROLE_COLOR"), true, t("USE_ROLE_COLOR_SUBTITLE"))
appearance:switch("useEnhancedRoleColors", t("USE_ENHANCED_ROLE_COLORS"), true, t("USE_ENHANCED_ROLE_COLORS_SUBTITLE"))
appearance:switch("useOpTagStyle", t("USE_OP_TAG_STYLE"), false, t("USE_OP_TAG_STYLE_SUBTITLE"))

local role_name = astral.settings.section(t("ROLE_NAME"))
role_name:switch("showRoleName", t("SHOW_ROLE_NAME"), false, t("SHOW_ROLE_NAME_SUBTITLE"))
role_name:switch("roleNameOnBots", t("ROLE_NAME_ON_BOTS"), false, t("ROLE_NAME_ON_BOTS_SUBTITLE"))

local ROLE_COLOR_FIELDS = { "primary_color", "secondary_color", "tertiary_color" }

local function role_stops(role)
	local colors = role.colors or {}
	if (colors.primary_color or 0) == 0 then
		return {}
	end
	local fields = settings.useEnhancedRoleColors and ROLE_COLOR_FIELDS or { "primary_color" }
	local stops = {}
	for _, field in ipairs(fields) do
		local value = colors[field]
		if value and value ~= 0 then
			astral.list.push_unique(stops, Color.from(value):opaque())
		end
	end
	return stops
end

local function top_colored_role(node)
	local guild, member = node:guild(), node:member()
	if not guild or not member then
		return nil
	end
	local best
	for _, role_id in ipairs(member.roles or {}) do
		local role = guild.roles[role_id]
		local colored = role and role.colors and (role.colors.primary_color or 0) ~= 0
		if colored and (role.position or 0) > (best and best.position or -1) then
			best = role
		end
	end
	return best
end

local function resolve_tag(node)
	local highest = astral.list.find(TAGS, function(tag)
		return tag.applies(node)
	end)
	return highest and settings[highest.key] and highest or nil
end

local function tag_colors(tag, node)
	if settings.useRoleColor then
		local role = top_colored_role(node)
		local stops = role and role_stops(role) or {}
		if #stops > 0 then
			return stops
		end
	end
	return settings.coloredTags and { tag.color } or {}
end

local function role_name_pill(node)
	if not settings.showRoleName or (node:is_bot() and not settings.roleNameOnBots) then
		return nil
	end
	local role = top_colored_role(node)
	return role and Pill(role.name):gradient(role_stops(role)) or nil
end

astral.ui.decorate(astral.Target.MessageAuthor, function(node)
	if not node.user_id then
		return nil
	end
	local pills = {}
	local tag = resolve_tag(node)
	if tag then
		pills[#pills + 1] =
			Pill(tag.text):gradient(tag_colors(tag, node)):shaped(settings.useOpTagStyle and Shape.Op or Shape.Pill)
	end
	pills[#pills + 1] = role_name_pill(node)
	return pills
end)

astral.log("ready")
