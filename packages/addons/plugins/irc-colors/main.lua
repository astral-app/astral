local xxhash64 = require("lib.xxhash64")
local t = astral.i18n.t

astral.settings.number("lightness", t("LIGHTNESS"), { min = 0, max = 100, step = 5, default = 70 })
astral.settings.number("saturation", t("SATURATION"), { min = 0, max = 100, step = 5, default = 100 })
astral.settings.text("seed", t("SEED"), "", { subtitle = t("SEED_SUBTITLE") })
astral.settings.switch(
	"onlyWithoutColor",
	t("ONLY_WITHOUT_COLOR"),
	false,
	{ subtitle = t("ONLY_WITHOUT_COLOR_SUBTITLE") }
)
astral.settings.switch("onlyInDms", t("ONLY_IN_DMS"), false, { subtitle = t("ONLY_IN_DMS_SUBTITLE") })

local colors = {}

local function hue(user_id, seed)
	return xxhash64.unsigned_modulo(xxhash64.hash(user_id .. seed), 360)
end

local function channel(lightness, saturation, offset, degrees)
	local s, l = saturation / 100, lightness / 100
	local chroma = s * math.min(l, 1 - l)
	local k = (offset + degrees / 30) % 12
	return math.floor(255 * (l - chroma * math.max(-1, math.min(k - 3, 9 - k, 1))) + 0.5)
end

local function user_color(user_id)
	local values = astral.settings.values
	local seed = values.seed or ""
	local lightness = values.lightness or 70
	local saturation = values.saturation or 100
	local key = table.concat({ user_id, seed, lightness, saturation }, ":")
	if colors[key] then
		return colors[key]
	end
	local degrees = hue(user_id, seed)
	local rgb = channel(lightness, saturation, 0, degrees) << 16
		| channel(lightness, saturation, 8, degrees) << 8
		| channel(lightness, saturation, 4, degrees)
	colors[key] = 0xFF000000 | rgb
	return colors[key]
end

local function has_role_color(guild_id, user_id)
	local guild = astral.discord.guild(guild_id)
	local member = astral.discord.member(guild_id, user_id)
	if not guild or not guild.roles or not member then
		return false
	end
	for _, id in ipairs(member.roles or {}) do
		local role = guild.roles[id]
		if role and role.colors and (role.colors.primary_color or 0) > 0 then
			return true
		end
	end
	return false
end

astral.ui.patch({ id = "message.author:", type = astral.PatchNodeType.Text }, function(node)
	local values = astral.settings.values
	if not node.author_id then
		return nil
	end
	local guild_id = astral.discord.channel_guild(node.channel_id or "")
	if guild_id and values.onlyInDms then
		return nil
	end
	if guild_id and values.onlyWithoutColor and has_role_color(guild_id, node.author_id) then
		return nil
	end
	local color = user_color(node.author_id)
	return color and { text_color = color } or nil
end)
