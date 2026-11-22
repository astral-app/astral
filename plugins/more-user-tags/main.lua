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

local function withPermissions(...)
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
	Tag("ADMINISTRATOR", "#DA373C", withPermissions(Permission.ADMINISTRATOR)),
	Tag(
		"MODERATOR_STAFF",
		"#248046",
		withPermissions(
			Permission.MANAGE_GUILD,
			Permission.MANAGE_CHANNELS,
			Permission.MANAGE_ROLES,
			Permission.MANAGE_WEBHOOKS
		)
	),
	Tag(
		"MODERATOR",
		"#4E7FFF",
		withPermissions(Permission.MANAGE_MESSAGES, Permission.KICK_MEMBERS, Permission.BAN_MEMBERS)
	),
	Tag(
		"VOICE_MODERATOR",
		"#059669",
		withPermissions(Permission.MOVE_MEMBERS, Permission.MUTE_MEMBERS, Permission.DEAFEN_MEMBERS)
	),
	Tag("CHAT_MODERATOR", "#C27C0E", withPermissions(Permission.MODERATE_MEMBERS)),
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

local roleName = astral.settings.section(t("ROLE_NAME"))
roleName:switch("showRoleName", t("SHOW_ROLE_NAME"), false, t("SHOW_ROLE_NAME_SUBTITLE"))
roleName:switch("roleNameOnBots", t("ROLE_NAME_ON_BOTS"), false, t("ROLE_NAME_ON_BOTS_SUBTITLE"))

local ROLE_COLOR_FIELDS = { "primary_color", "secondary_color", "tertiary_color" }

local function roleStops(role)
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

local function topColoredRole(node)
	local guild, member = node:guild(), node:member()
	if not guild or not member then
		return nil
	end
	local best
	for _, roleId in ipairs(member.roles or {}) do
		local role = guild.roles[roleId]
		local colored = role and role.colors and (role.colors.primary_color or 0) ~= 0
		if colored and (role.position or 0) > (best and best.position or -1) then
			best = role
		end
	end
	return best
end

local function resolveTag(node)
	return astral.list.find(TAGS, function(tag)
		return settings[tag.key] and tag.applies(node)
	end)
end

local function tagColors(tag, node)
	if settings.useRoleColor then
		local role = topColoredRole(node)
		local stops = role and roleStops(role) or {}
		if #stops > 0 then
			return stops
		end
	end
	return settings.coloredTags and { tag.color } or {}
end

local function roleNamePill(node)
	if not settings.showRoleName or (node:is_bot() and not settings.roleNameOnBots) then
		return nil
	end
	local role = topColoredRole(node)
	return role and Pill(role.name):gradient(roleStops(role)) or nil
end

astral.ui.decorate(astral.Target.MessageAuthor, function(node)
	if not node.user_id then
		return nil
	end
	local pills = {}
	local tag = resolveTag(node)
	if tag then
		pills[#pills + 1] =
			Pill(tag.text):gradient(tagColors(tag, node)):shaped(settings.useOpTagStyle and Shape.Op or Shape.Pill)
	end
	pills[#pills + 1] = roleNamePill(node)
	return pills
end)

astral.log("ready")
