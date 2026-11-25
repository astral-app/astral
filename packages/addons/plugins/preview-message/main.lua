local COMPOSER_ICON = 0xFFABACB2

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

local composing = false

astral.ui.composer(function(text)
	local now = text ~= ""
	if now ~= composing then
		composing = now
		astral.time.after(0, function()
			astral.ui.refresh()
		end)
	end
	return {}
end)

astral.ui.insert({ id = "composer.expressions" }, function()
	if not composing then
		return nil
	end
	return {
		type = "view",
		row = true,
		justify = "center",
		width = 32,
		height = 32,
		radius = 8,
		position = "start",
		action = "preview",
		children = {
			{ type = "image", icon = "res://ic_mana_eye", width = 20, height = 20, color = COMPOSER_ICON },
		},
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
