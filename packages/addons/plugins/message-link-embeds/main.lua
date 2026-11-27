local MAXIMUM_EMBEDS = 3
local SNIPPET = 600
local SURFACE = 0xFF2B2D31
local NAME = 0xFFF2F3F5
local TEXT = 0xFFDBDEE1
local MUTED = 0xFF949BA4
local AVATAR = 40
local IMAGE_WIDTH = 240
local IMAGE_HEIGHT = 240
local CDN = "https://cdn.discordapp.com/"
local TAG_SURFACE = 0xFF3F4147
local APP_TAG = 0xFF5865F2

local t = astral.i18n.t

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

local function avatar_url(author)
	if type(author.avatar) == "string" and author.avatar ~= "" then
		return string.format("%savatars/%s/%s.png?size=80", CDN, author.id, author.avatar)
	end
	local index = author.id and (tonumber(author.id:sub(-6)) or 0) % 6 or 0
	return string.format("%sembed/avatars/%d.png", CDN, index)
end

local function readable(content)
	return (
		tostring(content or ""):gsub("<@!?(%d+)>", function(id)
			local user = astral.discord.user(id)
			return "@" .. (user and (user.global_name or user.username) or id)
		end)
	)
end

local function body(content)
	local text = readable(content)
	if #text <= SNIPPET then
		return text
	end
	local cut = text:sub(1, SNIPPET)
	local open = cut:match(".*()<a?:[%w_]*:?%d*$")
	if open then
		cut = cut:sub(1, open - 1)
	end
	while #cut > 0 and cut:byte(#cut) >= 0x80 and cut:byte(#cut) < 0xC0 do
		cut = cut:sub(1, -2)
	end
	if #cut > 0 and cut:byte(#cut) >= 0xC0 then
		cut = cut:sub(1, -2)
	end
	return cut .. "…"
end

local function role_color(guild_id, user_id)
	if not guild_id or guild_id == "@me" or not user_id then
		return NAME
	end
	local guild = astral.discord.guild(guild_id)
	local member = guild and astral.discord.member(guild_id, user_id)
	if not guild or not member then
		return NAME
	end
	local best, color = -1, NAME
	for _, role_id in ipairs(member.roles or {}) do
		local role = guild.roles[role_id]
		local value = role and role.colors and tonumber(role.colors.primary_color) or 0
		local position = role and tonumber(role.position) or -1
		if value ~= 0 and position > best then
			best, color = position, 0xFF000000 + value
		end
	end
	return color
end

local function guild_tag(author)
	local tag = type(author.primary_guild) == "table" and author.primary_guild or nil
	if not tag or tag.identity_enabled == false or type(tag.tag) ~= "string" or tag.tag == "" then
		return nil
	end
	local children = {}
	if type(tag.badge) == "string" and type(tag.identity_guild_id) == "string" then
		children[1] = {
			type = astral.InsertNodeType.Image,
			icon = string.format("%sguild-tag-badges/%s/%s.png?size=32", CDN, tag.identity_guild_id, tag.badge),
			width = 12,
			height = 12,
			margin = { 0, 0, 3, 0 },
		}
	end
	children[#children + 1] =
		{ type = astral.InsertNodeType.Text, text = tag.tag, size = 12, weight = 600, color = NAME }
	return {
		type = astral.InsertNodeType.View,
		row = true,
		align = astral.Align.Center,
		background = TAG_SURFACE,
		radius = 4,
		padding = { 4, 1, 4, 1 },
		margin = { 6, 0, 0, 0 },
		children = children,
	}
end

local function app_tag(author)
	if not author.bot then
		return nil
	end
	return {
		type = astral.InsertNodeType.View,
		background = APP_TAG,
		radius = 4,
		padding = { 4, 1, 4, 1 },
		margin = { 6, 0, 0, 0 },
		children = { { type = astral.InsertNodeType.Text, text = t("APP"), size = 11, weight = 600, color = NAME } },
	}
end

local function timestamp(message)
	return astral.time.iso(astral.snowflake.timestamp(message.id), { local_time = true })
end

local function first_image(message)
	for _, attachment in ipairs(message.attachments or {}) do
		local kind = tostring(attachment.content_type or "")
		if kind:sub(1, 6) == "image/" and type(attachment.url) == "string" then
			return attachment
		end
	end
	return nil
end

local function image_node(attachment)
	local width, height = tonumber(attachment.width) or IMAGE_WIDTH, tonumber(attachment.height) or IMAGE_HEIGHT
	local scale = math.min(1, IMAGE_WIDTH / math.max(width, 1), IMAGE_HEIGHT / math.max(height, 1))
	return {
		type = astral.InsertNodeType.Image,
		icon = attachment.proxy_url or attachment.url,
		width = math.floor(width * scale),
		height = math.floor(height * scale),
		radius = 8,
		margin = { 0, 6, 0, 0 },
	}
end

local function header(link, linked, author)
	local children = {
		{
			type = astral.InsertNodeType.Text,
			text = author_name(linked),
			size = 16,
			weight = 600,
			color = role_color(link.guild, author.id),
		},
	}
	children[#children + 1] = app_tag(author)
	children[#children + 1] = guild_tag(author)
	children[#children + 1] = {
		type = astral.InsertNodeType.Text,
		text = timestamp(linked),
		size = 12,
		color = MUTED,
		margin = { 6, 0, 0, 0 },
	}
	return { type = astral.InsertNodeType.View, row = true, align = astral.Align.Center, children = children }
end

local function card(link)
	local linked = astral.discord.raw_message(link.channel, link.message)
	if type(linked) ~= "table" then
		return nil
	end
	linked.id = linked.id or link.message
	local author = type(linked.author) == "table" and linked.author or {}
	local column = {
		type = astral.InsertNodeType.View,
		children = { header(link, linked, author) },
	}
	local text = body(linked.content)
	if text ~= "" then
		column.children[#column.children + 1] =
			{ type = astral.InsertNodeType.Text, text = text, size = 16, color = TEXT, fill = true }
	end
	local image = first_image(linked)
	if image then
		column.children[#column.children + 1] = image_node(image)
	end
	return {
		type = astral.InsertNodeType.View,
		row = true,
		align = astral.Align.Start,
		background = SURFACE,
		radius = 8,
		padding = { 12, 10, 12, 10 },
		margin = { 0, 6, 0, 0 },
		fill = true,
		action = link.channel .. ":" .. link.message,
		children = {
			{
				type = astral.InsertNodeType.Image,
				icon = avatar_url(author),
				width = AVATAR,
				height = AVATAR,
				radius = AVATAR / 2,
				margin = { 0, 2, 12, 0 },
			},
			column,
		},
	}
end

local RETRY_MS = 500
local RETRIES = 6
local retries = {}

local function retry_later(message_id)
	local count = retries[message_id] or 0
	if count >= RETRIES then
		return
	end
	retries[message_id] = count + 1
	astral.time.after(RETRY_MS, function()
		astral.ui.refresh()
	end)
end

astral.ui.insert({ id = "message.content:" }, function(node)
	local message = node.message_id and node.channel_id and astral.discord.message(node.channel_id, node.message_id)
	if node.message_id and not message then
		retry_later(node.message_id)
		return nil
	end
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
