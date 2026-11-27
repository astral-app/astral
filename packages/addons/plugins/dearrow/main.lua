local BRANDING_URL = "https://sponsor.ajay.app/api/branding?videoID="
local THUMBNAIL_URL = "https://dearrow-thumb.ajay.app/api/v1/getThumbnail?videoID=%s&time=%s"
local REMEMBERED = 500
local t = astral.i18n.t

local section = astral.settings.section(t("REPLACE"))
section:choice("replace", t("REPLACE"), {
	astral.settings.option("all", t("REPLACE_ALL")),
	astral.settings.option("titles", t("REPLACE_TITLES")),
	astral.settings.option("thumbnails", t("REPLACE_THUMBNAILS")),
}, "all")
section:switch("by_default", t("DEARROW_BY_DEFAULT"), true, t("DEARROW_BY_DEFAULT_SUBTITLE"))
section:switch("show_toggle", t("SHOW_TOGGLE"), true, t("SHOW_TOGGLE_SUBTITLE"))

local branding = {}
local pending = {}
local messages = {}
local order = {}
local flipped = {}

local function video_id(url)
	if type(url) ~= "string" then
		return nil
	end
	local host, path = url:match("^https?://([^/]+)(/[^#]*)")
	if not host then
		return nil
	end
	host = host:gsub("^www%.", ""):gsub("^m%.", ""):gsub("^music%.", "")
	local id
	if host == "youtu.be" then
		id = path:match("^/([%w_-]+)")
	elseif host == "youtube.com" or host == "youtube-nocookie.com" then
		id = path:match("^/embed/([%w_-]+)")
			or path:match("^/shorts/([%w_-]+)")
			or path:match("^/live/([%w_-]+)")
			or path:match("^/v/([%w_-]+)")
		if not id and path:match("^/watch%?") then
			id = path:match("[?&]v=([%w_-]+)")
		end
	end
	return id and #id == 11 and id or nil
end

local function embed_video(embed)
	if type(embed) ~= "table" or type(embed.provider) ~= "table" or embed.provider.name ~= "YouTube" then
		return nil
	end
	return video_id(type(embed.video) == "table" and embed.video.url or nil) or video_id(embed.url)
end

local function clean_title(title)
	return (" " .. title):gsub("(%s)>(%S)", "%1%2"):sub(2)
end

local function parse(id, response)
	if type(response) ~= "table" then
		return false
	end
	local found = {}
	local title = type(response.titles) == "table" and response.titles[1]
	if
		type(title) == "table"
		and (tonumber(title.votes) or -1) >= 0
		and type(title.title) == "string"
		and title.title:match("%S")
	then
		found.title = clean_title(title.title)
	end
	local thumbnail = type(response.thumbnails) == "table" and response.thumbnails[1]
	if
		type(thumbnail) == "table"
		and (tonumber(thumbnail.votes) or -1) >= 0
		and not thumbnail.original
		and tonumber(thumbnail.timestamp)
	then
		found.thumbnail = THUMBNAIL_URL:format(id, tostring(thumbnail.timestamp))
	end
	return (found.title or found.thumbnail) and found or false
end

local function wanted(message_id)
	local on = astral.settings.get("by_default", true) == true
	if flipped[message_id] then
		on = not on
	end
	return on
end

local function apply(embed, found)
	local replace = astral.settings.get("replace", "all")
	local changed = false
	if found.title and replace ~= "thumbnails" and embed.title ~= found.title then
		embed.title = found.title
		changed = true
	end
	if
		found.thumbnail
		and replace ~= "titles"
		and type(embed.thumbnail) == "table"
		and embed.thumbnail.url ~= found.thumbnail
	then
		embed.thumbnail.url = found.thumbnail
		embed.thumbnail.proxy_url = found.thumbnail
		changed = true
	end
	return changed
end

local function copy(value)
	if type(value) ~= "table" then
		return value
	end
	local result = {}
	for key, item in pairs(value) do
		result[key] = copy(item)
	end
	return result
end

local function remember(message)
	if not messages[message.id] then
		order[#order + 1] = message.id
		if #order > REMEMBERED then
			messages[table.remove(order, 1)] = nil
		end
	end
	messages[message.id] = { channel_id = message.channel_id, embeds = copy(message.embeds) }
end

local function rebrand(message, missing)
	if type(message) ~= "table" or type(message.embeds) ~= "table" or not message.id then
		return false
	end
	local videos = false
	for _, embed in ipairs(message.embeds) do
		videos = videos or embed_video(embed) ~= nil
	end
	if not videos then
		return false
	end
	remember(message)
	if not wanted(message.id) then
		return false
	end
	local changed = false
	for _, embed in ipairs(message.embeds) do
		local id = embed_video(embed)
		if id and branding[id] then
			changed = apply(embed, branding[id]) or changed
		elseif id and branding[id] == nil and missing then
			missing[id] = true
		end
	end
	return changed
end

local REDRAWN = "astral_dearrow"

local function redraw(message_id)
	local stored = messages[message_id]
	if not stored then
		return
	end
	local message = { id = message_id, channel_id = stored.channel_id, embeds = copy(stored.embeds) }
	if wanted(message_id) then
		for _, embed in ipairs(message.embeds) do
			local id = embed_video(embed)
			if id and branding[id] then
				apply(embed, branding[id])
			end
		end
	end
	message[REDRAWN] = true
	astral.gateway.dispatch("MESSAGE_UPDATE", message)
end

local function fetch(id, done)
	if pending[id] then
		pending[id][#pending[id] + 1] = done
		return
	end
	pending[id] = { done }
	astral.net.request({ url = BRANDING_URL .. id, timeout_ms = 10000 }, function(response)
		branding[id] = response and response.status == 200 and parse(id, response.json) or false
		local waiting = pending[id]
		pending[id] = nil
		for _, callback in ipairs(waiting) do
			callback()
		end
	end)
end

local function redraw_videos(missing)
	for id in pairs(missing) do
		fetch(id, function()
			if not branding[id] then
				return
			end
			for message_id, stored in pairs(messages) do
				for _, embed in ipairs(stored.embeds or {}) do
					if embed_video(embed) == id then
						redraw(message_id)
						break
					end
				end
			end
		end)
	end
end

astral.events.on(astral.Event.Gateway, function(event)
	if type(event.data) == "table" and event.data[REDRAWN] then
		event.data[REDRAWN] = nil
		return event.data
	end
	local missing = {}
	local changed = rebrand(event.data, missing)
	if next(missing) then
		redraw_videos(missing)
	end
	return changed and event.data or nil
end, { "MESSAGE_CREATE", "MESSAGE_UPDATE" })

local held = {}

astral.net.intercept({
	method = "GET",
	path = "^/api/v%d+/channels/%d+/messages$",
	response = function(_, response)
		if response.status ~= 200 then
			return nil
		end
		local list = astral.json.decode(response.body)
		if type(list) ~= "table" then
			return nil
		end
		local changed = false
		for _, message in ipairs(list) do
			changed = rebrand(message, held) or changed
		end
		if next(held) then
			local missing = held
			held = {}
			redraw_videos(missing)
		end
		return changed and { body = astral.json.encode(list) } or nil
	end,
})

local function has_video(node)
	return astral.settings.get("show_toggle", true) == true and messages[node.message_id] ~= nil
end

astral.ui.message_action(t("SHOW_ORIGINAL_EMBED"), function(node)
	flipped[node.message_id] = not flipped[node.message_id]
	redraw(node.message_id)
end, {
	icon = "res://ic_mana_refresh",
	when = function(node)
		return has_video(node) and wanted(node.message_id)
	end,
})

astral.ui.message_action(t("DEARROW_EMBED"), function(node)
	flipped[node.message_id] = not flipped[node.message_id]
	redraw(node.message_id)
end, {
	icon = "res://ic_mana_image",
	when = function(node)
		return has_video(node) and not wanted(node.message_id)
	end,
})
