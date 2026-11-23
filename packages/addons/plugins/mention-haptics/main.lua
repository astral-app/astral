local settings = astral.settings.section("Mention Haptics")

settings:choice("strength", "Haptic strength", {
	astral.settings.option("light", "Light"),
	astral.settings.option("medium", "Medium"),
	astral.settings.option("heavy", "Heavy"),
}, "medium")
settings:switch("directMessages", "Direct messages", true, "Tap for incoming direct messages.")
settings:switch("openChannel", "Current channel", false, "Tap when the message is in the open channel.")

local seen = {}
local recent = {}

local function remember(messageID)
	if not messageID then
		return true
	end
	if seen[messageID] then
		return false
	end
	seen[messageID] = true
	recent[#recent + 1] = messageID
	if #recent > 256 then
		seen[table.remove(recent, 1)] = nil
	end
	return true
end

astral.events.on("message.create", function(message)
	if not message or message.from_me or (message.open and not astral.settings.values.openChannel) then
		return
	end
	if not message.mentioned and not (message.direct and astral.settings.values.directMessages) then
		return
	end
	if not remember(message.message_id) then
		return
	end
	astral.ui.haptic(astral.settings.values.strength or "medium")
end)
