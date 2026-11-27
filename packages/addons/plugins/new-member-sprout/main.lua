local t = astral.i18n.t

astral.settings.switch("overRoleIcons", t("OVER_ROLE_ICONS"), false, { subtitle = t("OVER_ROLE_ICONS_SUBTITLE") })

local DAY_MS = 24 * 60 * 60 * 1000
local WINDOW_MS = 7 * DAY_MS
local DISCORD_EPOCH = 1420070400000
local DID_REJOIN = 1

local function days_from_civil(year, month, day)
	year = month <= 2 and year - 1 or year
	local era = (year >= 0 and year or year - 399) // 400
	local year_of_era = year - era * 400
	local day_of_year = (153 * (month + (month > 2 and -3 or 9)) + 2) // 5 + day - 1
	local day_of_era = year_of_era * 365 + year_of_era // 4 - year_of_era // 100 + day_of_year
	return era * 146097 + day_of_era - 719468
end

local function parse_timestamp(text)
	local year, month, day, hour, minute, second = tostring(text or ""):match("^(%d+)-(%d+)-(%d+)T(%d+):(%d+):(%d+)")
	if not year then
		return nil
	end
	local sign, offset_hours, offset_minutes = text:match("([+-])(%d%d):(%d%d)$")
	local offset = sign and (tonumber(offset_hours) * 60 + tonumber(offset_minutes)) * 60 * (sign == "-" and -1 or 1)
		or 0
	local seconds = days_from_civil(tonumber(year), tonumber(month), tonumber(day)) * 86400
		+ tonumber(hour) * 3600
		+ tonumber(minute) * 60
		+ tonumber(second)
		- offset
	return seconds * 1000
end

local function guild_created_at(guild_id)
	return (math.tointeger(guild_id) >> 22) + DISCORD_EPOCH
end

local function is_new_member(guild_id, member, now)
	local joined = member and parse_timestamp(member.joined_at)
	if not joined or joined > now or now - joined > WINDOW_MS then
		return false
	end
	if ((member.flags or 0) & DID_REJOIN) ~= 0 then
		return false
	end
	return now - guild_created_at(guild_id) >= WINDOW_MS
end

local function has_role_icon(node, member)
	local guild = node:guild()
	for _, role_id in ipairs(member and member.roles or {}) do
		local role = guild and guild.roles[role_id]
		if role and type(role.icon) == "string" and role.icon ~= "" then
			return true
		end
	end
	return false
end

astral.ui.decorate(astral.Target.MessageAuthor, function(node)
	if not node.user_id or not node.guild_id or node:is_bot() then
		return nil
	end
	local member = node:member()
	if not is_new_member(node.guild_id, member, astral.time.now_ms()) then
		return nil
	end
	if has_role_icon(node, member) and not astral.settings.values.overRoleIcons then
		return nil
	end
	return astral.Pill(t("NEW_HERE"), { icon = "sprout.png" }):on_press("sprout")
end, function()
	astral.ui.tooltip(t("NEW_HERE"))
end)
