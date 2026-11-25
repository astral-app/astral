local LIMIT = 1900

local function fenced(language, text)
	text = text:gsub("```", "`\u{200b}``")
	if #text > LIMIT then
		text = text:sub(1, LIMIT) .. "\n…"
	end
	return "```" .. language .. "\n" .. text .. "\n```"
end

local function show_raw(node)
	local message = node:message()
	if not message then
		astral.ui.toast("View Raw", "This message isn't cached yet")
		return
	end
	local content = type(message.content) == "string" and message.content or ""
	local data = astral.json.encode(message)
	astral.messages.send_local(node.channel_id, {
		content = (content ~= "" and fenced("md", content) .. "\n" or "") .. fenced("json", data),
	})
end

local function copy_raw(node)
	local message = node:message()
	if message and type(message.content) == "string" then
		astral.ui.copy(message.content)
		astral.ui.toast("View Raw", "Copied the raw content")
	end
end

astral.ui.message_action("View Raw", show_raw, { icon = "res://ic_mana_code" })
astral.ui.message_action("Copy Raw", copy_raw, {
	icon = "res://ic_mana_copy",
	when = function(node)
		local message = node:message()
		return message ~= nil and type(message.content) == "string" and message.content ~= ""
	end,
})
