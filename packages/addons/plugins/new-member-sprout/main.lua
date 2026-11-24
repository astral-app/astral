local DAY_MS = 24 * 60 * 60 * 1000
local WINDOW_MS = 7 * DAY_MS
local DISCORD_EPOCH = 1420070400000
local DID_REJOIN = 1

local function daysFromCivil(year, month, day)
	year = month <= 2 and year - 1 or year
	local era = (year >= 0 and year or year - 399) // 400
	local yearOfEra = year - era * 400
	local dayOfYear = (153 * (month + (month > 2 and -3 or 9)) + 2) // 5 + day - 1
	local dayOfEra = yearOfEra * 365 + yearOfEra // 4 - yearOfEra // 100 + dayOfYear
	return era * 146097 + dayOfEra - 719468
end

local function parseTimestamp(text)
	local year, month, day, hour, minute, second = tostring(text or ""):match("^(%d+)-(%d+)-(%d+)T(%d+):(%d+):(%d+)")
	if not year then
		return nil
	end
	local sign, offsetHours, offsetMinutes = text:match("([+-])(%d%d):(%d%d)$")
	local offset = sign and (tonumber(offsetHours) * 60 + tonumber(offsetMinutes)) * 60 * (sign == "-" and -1 or 1) or 0
	local seconds = daysFromCivil(tonumber(year), tonumber(month), tonumber(day)) * 86400
		+ tonumber(hour) * 3600
		+ tonumber(minute) * 60
		+ tonumber(second)
		- offset
	return seconds * 1000
end

local function guildCreatedAt(guildId)
	return (math.tointeger(guildId) >> 22) + DISCORD_EPOCH
end

local function isNewMember(guildId, member, now)
	local joined = member and parseTimestamp(member.joined_at)
	if not joined or joined > now or now - joined > WINDOW_MS then
		return false
	end
	if ((member.flags or 0) & DID_REJOIN) ~= 0 then
		return false
	end
	return now - guildCreatedAt(guildId) >= WINDOW_MS
end

astral.ui.decorate(astral.Target.MessageAuthor, function(node)
	if not node.user_id or not node.guild_id then
		return nil
	end
	if not isNewMember(node.guild_id, node:member(), astral.time.now_ms()) then
		return nil
	end
	return astral.Pill(astral.i18n.t("NEW_MEMBER"), { icon = "sprout.png" })
end)
