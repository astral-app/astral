local KLIPY_API_KEY = "685pfsUU3EODe5rjG3li8rLUdfyydxxfh8fPym7wM5dvr0jklulSi6g5BSWlL3zG"
local KLIPY_ITEM = "https://api.klipy.com/api/v1/%s/gifs/%s"
local KLIPY_SEARCH = "https://api.klipy.com/api/v1/%s/gifs/search?per_page=30&q=%s&page=1&customer_id=astral"
local PROXY_HOSTS = { "^images%-ext%-%d+%.discordapp%.net$", "^media%.discordapp%.net$" }

local resolved = {}

local function split(url)
	local scheme, host, path, query = url:match("^(https?)://([^/?#]+)([^?#]*)%??([^#]*)")
	if not scheme then
		return nil
	end
	return { host = host:lower(), path = path, query = query }
end

local function query_value(parts, name)
	for pair in (parts.query or ""):gmatch("[^&]+") do
		local key, value = pair:match("^([^=]+)=?(.*)$")
		if key == name then
			return value:lower()
		end
	end
	return nil
end

local function with_query_value(url, name, value)
	local base, query = url:match("^([^?#]*)%??([^#]*)")
	local parts, found = {}, false
	for pair in query:gmatch("[^&]+") do
		if pair:match("^([^=]+)") == name then
			parts[#parts + 1] = name .. "=" .. value
			found = true
		else
			parts[#parts + 1] = pair
		end
	end
	if not found then
		parts[#parts + 1] = name .. "=" .. value
	end
	return base .. "?" .. table.concat(parts, "&")
end

local function unescape(text)
	return (text:gsub("%%(%x%x)", function(hex)
		return string.char(tonumber(hex, 16))
	end))
end

local function is_proxy(parts)
	if not parts or not parts.path:lower():find("/external/", 1, true) then
		return false
	end
	for _, pattern in ipairs(PROXY_HOSTS) do
		if parts.host:match(pattern) then
			return true
		end
	end
	return false
end

local function proxy_target(url)
	local parts = split(url)
	if not parts or not is_proxy(parts) then
		return nil
	end
	local path = unescape(parts.path)
	local start = path:find("/https/", 1, true)
	if not start then
		return nil
	end
	local prefix = path:sub(#"/external/" + 1, start - 1)
	local target = "https://" .. path:sub(start + #"/https/")
	return prefix:sub(1, 1) == "?" and target .. prefix or target
end

local function is_provider(text)
	local lower = text:lower()
	return lower:find("klipy", 1, true) or lower:find("tenor", 1, true) or lower:find("giphy", 1, true)
end

local function is_provider_video(url)
	local parts = split(url)
	if not parts then
		return false
	end
	local path = parts.path:lower()
	local format = query_value(parts, "format")
	return is_provider(parts.host .. path .. parts.query)
		and (
			path:match("%.mp4$")
			or path:match("%.webm$")
			or path:match("%.webp$")
			or format == "mp4"
			or format == "video"
			or format == "webp"
		)
end

local function canonical_giphy(url, parts)
	local id = parts.path:match("/media/([^/]+)") or parts.path:match("/gifs/[^/]-([%w]+)$")
	return id and ("https://media.giphy.com/media/" .. id .. "/giphy.gif") or url
end

local function direct_gif(url)
	local parts = split(url)
	if not parts then
		return nil
	end
	local path = parts.path:lower()
	if is_proxy(parts) then
		local target = proxy_target(url)
		if target then
			return direct_gif(target)
		end
		return path:match("%.gif$") and url or nil
	end
	if path:match("%.gif$") then
		local format = query_value(parts, "format")
		return (format and format ~= "gif") and with_query_value(url, "format", "gif") or url
	end
	local extension = path:match("%.(%w+)$")
	if
		query_value(parts, "animated") == "true" and (extension == "webp" or extension == "avif" or extension == "mp4")
	then
		local gif = url:gsub("%." .. extension .. "([?#]?)", ".gif%1", 1)
		return with_query_value(gif, "format", "gif")
	end
	if parts.host:find("giphy.com", 1, true) and parts.path:lower():match("/gifs?/") then
		local canonical = canonical_giphy(url, parts)
		if canonical ~= url then
			return canonical
		end
	end
	if parts.host == "tenor.com" then
		return "https://tenor.com" .. parts.path .. ".gif" .. (parts.query ~= "" and ("?" .. parts.query) or "")
	end
	return nil
end

local function as_gif(url)
	local target = proxy_target(url)
	if target then
		return as_gif(target)
	end
	local direct = direct_gif(url)
	if direct then
		return direct
	end
	if not is_provider_video(url) then
		return nil
	end
	local parts = split(url)
	if not parts then
		return nil
	end
	if parts.host:find("giphy.com", 1, true) then
		return canonical_giphy(url, parts)
	end
	if parts.host:find("klipy.com", 1, true) then
		return nil
	end
	local gif =
		url:gsub("%.mp4([?#]?)", ".gif%1", 1):gsub("%.webm([?#]?)", ".gif%1", 1):gsub("%.webp([?#]?)", ".gif%1", 1)
	return query_value(parts, "format") and with_query_value(gif, "format", "gif") or gif
end

local function media_folder(url)
	local parts = split(proxy_target(url) or url)
	return parts and parts.path:match("/ii/(.+)/[^/]+$") or nil
end

local function page_slug(url)
	local parts = split(url)
	if not parts or not parts.host:match("klipy%.com$") then
		return nil
	end
	local slug = parts.path:match("^/gifs/([^/]+)")
	return slug and unescape(slug) or nil
end

local function find_gif(value, depth)
	if depth > 7 then
		return nil
	end
	if type(value) == "string" then
		local lower = value:lower()
		return (lower:match("%.gif$") or lower:match("%.gif[?#]")) and lower:find("klipy", 1, true) and value or nil
	end
	if type(value) ~= "table" then
		return nil
	end
	for _, key in ipairs({ "gif", "url" }) do
		local found = find_gif(value[key], depth + 1)
		if found then
			return found
		end
	end
	for _, nested in pairs(value) do
		local found = find_gif(nested, depth + 1)
		if found then
			return found
		end
	end
	return nil
end

local function pick_klipy_gif(results, slug, folder)
	for _, result in ipairs(results) do
		local url = find_gif(result, 0)
		if url and folder and media_folder(url) == folder then
			return url
		end
	end
	for _, result in ipairs(results) do
		local url = find_gif(result, 0)
		local candidate = type(result) == "table" and result.slug or nil
		if
			url
			and slug
			and type(candidate) == "string"
			and (candidate == slug or candidate:sub(1, #slug + 2) == slug .. "--")
		then
			return url
		end
	end
	return nil
end

local function message_slug(save)
	if not save.channel_id or not save.message_id then
		return nil
	end
	local message = astral.discord.raw_message(save.channel_id, save.message_id)
	for _, embed in ipairs(message and message.embeds or {}) do
		local slug = type(embed.url) == "string" and page_slug(embed.url)
		if slug then
			return slug
		end
		if type(embed.title) == "string" and embed.title ~= "" then
			return embed.title
		end
	end
	return message and type(message.content) == "string" and page_slug(message.content:match("https://%S+") or "")
		or nil
end

local function decoded(response)
	if not response or not response.ok then
		return nil
	end
	local payload = response.json or (response.body and astral.json.decode(response.body))
	return type(payload) == "table" and type(payload.data) == "table" and payload.data or nil
end

local function encoded(text)
	return (text:gsub("[^%w%-_.~]", function(character)
		return string.format("%%%02X", character:byte())
	end))
end

local function search_klipy(slug, folder, reply)
	local query = encoded(slug:gsub("[-_]+", " "))
	astral.net.request({ url = KLIPY_SEARCH:format(KLIPY_API_KEY, query), timeout_ms = 10000 }, function(response)
		local data = decoded(response)
		local results = data and type(data.data) == "table" and data.data
		reply(results and pick_klipy_gif(results, slug, folder) or nil)
	end)
end

local function lookup_klipy(slug, folder, reply)
	astral.net.request({ url = KLIPY_ITEM:format(KLIPY_API_KEY, encoded(slug)), timeout_ms = 10000 }, function(response)
		local item = decoded(response)
		local gif = item and pick_klipy_gif({ item }, slug, folder)
		if gif then
			return reply(gif)
		end
		search_klipy(slug, folder, reply)
	end)
end

local function resolve_klipy(save, reply)
	local folder = media_folder(save.url)
	local slug = message_slug(save)
	local key = folder or slug
	if not key then
		return reply(nil)
	end
	if resolved[key] then
		return reply(resolved[key])
	end
	if not slug then
		return reply(nil)
	end
	lookup_klipy(slug, folder, function(gif)
		if gif then
			resolved[key] = gif
		end
		reply(gif)
	end)
end

local function is_klipy(url)
	local parts = split(proxy_target(url) or url)
	return parts and parts.host:find("klipy", 1, true) ~= nil
end

astral.media.on_save(function(save, reply)
	local gif = as_gif(save.url)
	if gif then
		return reply(gif)
	end
	if is_klipy(save.url) then
		return resolve_klipy(save, reply)
	end
	reply(nil)
end)
