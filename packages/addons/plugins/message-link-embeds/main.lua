local MAXIMUM_EMBEDS = 3
local SNIPPET = 220
local CARD = 0x26000000
local BAR = 0xFF5865F2
local MUTED = 0xFF949BA4

local function links_in(content)
	local links = {}
	for guild, channel, message in tostring(content or ""):gmatch("discord%.com/channels/([%d@me]+)/(%d+)/(%d+)") do
		links[#links + 1] = { guild = guild, channel = channel, message = message }
		if #links == MAXIMUM_EMBEDS then
			break
		end
	end
	return links
end

local function author_name(message)
	local author = message.author or {}
	return author.global_name or author.username or "Unknown"
end

local function snippet(content)
	local text = tostring(content or ""):gsub("%s+", " ")
	if #text > SNIPPET then
		text = text:sub(1, SNIPPET) .. "…"
	end
	return text ~= "" and text or "Attachment"
end

local function card(link)
	local linked = astral.discord.message(link.channel, link.message)
	if not linked then
		return nil
	end
	return {
		type = "view",
		row = true,
		background = CARD,
		radius = 8,
		margin = { 0, 6, 0, 0 },
		action = link.channel .. ":" .. link.message,
		children = {
			{ type = "view", background = BAR, width = 3, radius = 2 },
			{
				type = "view",
				padding = { 10, 8, 10, 8 },
				children = {
					{ type = "text", text = author_name(linked), bold = true, size = 14 },
					{ type = "text", text = snippet(linked.content), size = 14, color = MUTED },
				},
			},
		},
	}
end

astral.ui.insert({ id = "message.content:" }, function(node)
	local message = node.message_id and node.channel_id and astral.discord.message(node.channel_id, node.message_id)
	local cards = {}
	for _, link in ipairs(links_in(message and message.content)) do
		cards[#cards + 1] = card(link)
	end
	return #cards > 0 and cards or nil
end, function(_, action)
	local channel, message = action:match("^(%d+):(%d+)$")
	if channel then
		astral.ui.open_channel(channel, message)
	end
end)
