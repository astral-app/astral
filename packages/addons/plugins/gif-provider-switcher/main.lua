local TENOR_KEY = "3Z0688EVWYKH"
local GIPHY_KEY = "Gc7131jiJuvI7IdN0HZ1D7nh0ow5BU6g"
local LIMIT = 40

local settings = astral.settings.section("Provider")
settings:choice("provider", "GIF provider", {
	astral.settings.option("tenor", "Tenor"),
	astral.settings.option("giphy", "Giphy"),
	astral.settings.option("discord", "Discord's default"),
}, "tenor")

local function encode(text)
	return (
		tostring(text or ""):gsub("[^%w%-_%.~]", function(character)
			return string.format("%%%02X", character:byte())
		end)
	)
end

local function query_of(url, key)
	local value = url:match("[?&]" .. key .. "=([^&]*)")
	return value and value:gsub("+", " "):gsub("%%(%x%x)", function(hex)
		return string.char(tonumber(hex, 16))
	end) or nil
end

local function tenor_gif(item)
	local media = type(item.media) == "table" and item.media[1] or nil
	local gif = media and media.gif
	local mp4 = media and (media.mp4 or media.tinymp4) or gif
	if not gif or not mp4 or not gif.url or not item.itemurl then
		return nil
	end
	return {
		id = item.id,
		title = item.title or item.h1_title or "",
		url = item.itemurl,
		gif_src = gif.url,
		src = mp4.url,
		width = mp4.dims and mp4.dims[1] or 0,
		height = mp4.dims and mp4.dims[2] or 0,
		preview = mp4.preview or gif.preview or gif.url,
	}
end

local function giphy_gif(item)
	local images = item.images or {}
	local original = images.original or {}
	local mp4 = images.original_mp4 or original
	if not original.url then
		return nil
	end
	return {
		id = item.id,
		title = item.title or "",
		url = item.url,
		gif_src = original.url,
		src = mp4.mp4 or original.url,
		width = tonumber(original.width) or 0,
		height = tonumber(original.height) or 0,
		preview = (images.fixed_width_still or images.original_still or {}).url or original.url,
	}
end

local function provider_url(provider, kind, query)
	if provider == "tenor" then
		if kind == "trending" then
			return "https://api.tenor.com/v1/trending?media_filter=minimal&limit=" .. LIMIT .. "&key=" .. TENOR_KEY
		end
		return "https://api.tenor.com/v1/search?media_filter=minimal&limit="
			.. LIMIT
			.. "&key="
			.. TENOR_KEY
			.. "&q="
			.. encode(query)
	end
	if kind == "trending" then
		return "https://api.giphy.com/v1/gifs/trending?limit=" .. LIMIT .. "&api_key=" .. GIPHY_KEY
	end
	return "https://api.giphy.com/v1/gifs/search?limit=" .. LIMIT .. "&api_key=" .. GIPHY_KEY .. "&q=" .. encode(query)
end

local function gifs_from(provider, body)
	local gifs = astral.json.array()
	local items = provider == "tenor" and (body and body.results) or (body and body.data)
	for _, item in ipairs(items or {}) do
		local gif = provider == "tenor" and tenor_gif(item) or giphy_gif(item)
		if gif then
			gifs[#gifs + 1] = gif
		end
	end
	return gifs
end

local function answer(request, kind)
	local provider = astral.settings.get("provider", "tenor")
	if provider == "discord" then
		return nil
	end
	local query = query_of(request.url, "q") or ""
	if kind == "search" and query == "" then
		return nil
	end
	astral.net.request({ url = provider_url(provider, kind, query) }, function(response)
		if not response or tonumber(response.status) ~= 200 or not response.json then
			astral.net.release(request.id)
			return
		end
		astral.net.respond(request.id, {
			status = 200,
			headers = { ["content-type"] = "application/json" },
			body = astral.json.encode(gifs_from(provider, response.json)),
		})
	end)
	return astral.net.HOLD
end

astral.net.intercept({
	method = "GET",
	path = "^/api/v%d+/gifs/search$",
	request = function(request)
		return answer(request, "search")
	end,
})

astral.net.intercept({
	method = "GET",
	path = "^/api/v%d+/gifs/trending%-gifs$",
	request = function(request)
		return answer(request, "trending")
	end,
})
