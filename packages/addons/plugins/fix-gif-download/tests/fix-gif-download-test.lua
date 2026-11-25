local handler, requests = nil, {}
local message = {
	embeds = { { url = "https://klipy.com/gifs/kpop-triples-405", type = "gifv" } },
}
astral = {
	log = function() end,
	media = {
		on_save = function(fn)
			handler = fn
		end,
	},
	net = {
		request = function(options, callback)
			requests[#requests + 1] = { options = options, callback = callback }
		end,
	},
	json = {
		decode = function()
			return nil
		end,
	},
	discord = {
		raw_message = function(channel, id)
			return channel == "1" and id == "2" and message or nil
		end,
	},
}
dofile(arg[1])

local function saved(url, extra)
	local answer = "unanswered"
	local save = { url = url, kind = "video" }
	for key, value in pairs(extra or {}) do
		save[key] = value
	end
	handler(save, function(value)
		answer = value
	end)
	return answer
end

assert(saved("https://cdn.discordapp.com/attachments/1/2/clip.mp4") == nil)
assert(saved("https://media.tenor.com/abc/cat.mp4") == "https://media.tenor.com/abc/cat.gif")
assert(
	saved("https://images-ext-1.discordapp.net/external/x/https/media.tenor.com/abc/cat.mp4")
		== "https://media.tenor.com/abc/cat.gif"
)
assert(saved("https://media.giphy.com/media/xyz123/giphy.mp4") == "https://media.giphy.com/media/xyz123/giphy.gif")
assert(
	saved("https://media.discordapp.net/attachments/1/2/a.webp?animated=true")
		== "https://media.discordapp.net/attachments/1/2/a.gif?animated=true&format=gif"
)

local video = "https://images-ext-1.discordapp.net/external/U4j/https/static.klipy.com/ii/4e7b/8e/b9/qE3d.mp4"
assert(saved(video, { channel_id = "1", message_id = "2" }) == "unanswered")
local lookup = requests[#requests]
assert(lookup.options.url:find("/gifs/kpop-triples-405", 1, true))
lookup.callback({ ok = false })
local search = requests[#requests]
assert(search.options.url:find("q=kpop%20triples%20405", 1, true))
local answer
handler({ url = video, kind = "video", channel_id = "1", message_id = "2" }, function(value)
	answer = value
end)
assert(#requests == 3)
search.callback({
	ok = true,
	json = {
		data = {
			data = {
				{
					slug = "other",
					file = { hd = { gif = { url = "https://static.klipy.com/ii/ffff/00/00/other.gif" } } },
				},
				{
					slug = "kpop-triples-405",
					file = { hd = { gif = { url = "https://static.klipy.com/ii/4e7b/8e/b9/real.gif" } } },
				},
			},
		},
	},
})
requests[3].callback({ ok = false })
assert(answer == nil)
assert(saved(video, { channel_id = "1", message_id = "2" }) == "https://static.klipy.com/ii/4e7b/8e/b9/real.gif")
assert(saved("https://static.klipy.com/ii/9/9/9/x.mp4") == nil)
local fresh = "https://static.klipy.com/ii/aaaa/11/22/v.mp4"
assert(saved(fresh, { channel_id = "1", message_id = "2" }) == "unanswered")
requests[#requests].callback({
	ok = true,
	json = {
		data = {
			slug = "kpop-triples-405",
			file = { hd = { gif = { url = "https://static.klipy.com/ii/aaaa/11/22/g.gif" } } },
		},
	},
})
assert(saved(fresh, { channel_id = "1", message_id = "2" }) == "https://static.klipy.com/ii/aaaa/11/22/g.gif")
print("fix-gif-download ok")
