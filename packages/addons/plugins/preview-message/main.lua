local current_user

local function me()
	return current_user and astral.discord.user(current_user) or nil
end

local function preview(node)
	local text = astral.ui.composer_text()
	if text == "" then
		astral.ui.toast("Preview Message", "Type a message to preview it")
		return
	end
	local author = me()
	local message = { content = text }
	if author then
		message.author = author
	end
	astral.messages.send_local(node.channel_id, message)
end

astral.ui.insert({ id = "composer.expressions" }, function()
	return {
		type = "image",
		icon = "res://ic_mana_eye",
		width = 22,
		height = 22,
		margin = { 0, 0, 10, 0 },
		position = "start",
		action = "preview",
	}
end, function(node)
	preview(node)
end)

astral.events.on("gateway", function(event)
	if type(event.data.user) == "table" then
		current_user = event.data.user.id
	end
	return nil
end, { "READY" })
