local settings = astral.settings.section("Mention Haptics")

settings:choice("strength", "Haptic strength", {
	astral.settings.option("light", "Light"),
	astral.settings.option("medium", "Medium"),
	astral.settings.option("heavy", "Heavy"),
}, "medium")
settings:switch("direct_messages", "Direct messages", true, "Tap for incoming direct messages")
settings:switch("everyone", "@everyone and @here", true, "Tap when a message pings everyone in a server")
settings:switch("roles", "Your roles", true, "Tap when a message mentions a role you have")
settings:switch("open_channel", "Current channel", false, "Tap when the message is in the open channel")

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
	return message.mentioned
		or (message.direct and values.direct_messages)
		or (message.everyone and values.everyone)
		or (message.role_mentioned and values.roles)
end

astral.events.on("message.create", function(message)
	local values = astral.settings.values
	if not message or message.from_me or (message.open and not values.open_channel) then
		return
	end
	if not pings_me(message, values) or not remember(message.message_id) then
		return
	end
	astral.ui.haptic(values.strength or "medium")
end)
