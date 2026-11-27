local t = astral.i18n.t

astral.settings.choice("strength", t("STRENGTH"), {
	astral.settings.option("light", t("STRENGTH_LIGHT")),
	astral.settings.option("medium", t("STRENGTH_MEDIUM")),
	astral.settings.option("heavy", t("STRENGTH_HEAVY")),
}, "medium")
astral.settings.switch("directMessages", t("DIRECT_MESSAGES"), true, { subtitle = t("DIRECT_MESSAGES_SUBTITLE") })
astral.settings.switch("openChannel", t("OPEN_CHANNEL"), false, { subtitle = t("OPEN_CHANNEL_SUBTITLE") })

astral.settings.on_change("strength", function(strength)
	astral.ui.haptic(strength)
end)

local seen = {}
local recent = {}

local function remember(message_id)
	if not message_id then
		return true
	end
	if seen[message_id] then
		return false
	end
	seen[message_id] = true
	recent[#recent + 1] = message_id
	if #recent > 256 then
		seen[table.remove(recent, 1)] = nil
	end
	return true
end

local function pings_me(message, values)
	return message.mentioned or message.everyone or message.role_mentioned or (message.direct and values.directMessages)
end

astral.events.on(astral.Event.MessageCreate, function(message)
	local values = astral.settings.values
	if not message or message.from_me or (message.open and not values.openChannel) then
		return
	end
	if not pings_me(message, values) or not remember(message.message_id) then
		return
	end
	astral.ui.haptic(values.strength or "medium")
end)
