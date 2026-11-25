local intercepts, network, responded = {}, {}, nil
local values = {}
astral = {
	log = function() end,
	settings = {
		get = function(key, fallback)
			return values[key] or fallback
		end,
		option = function(value)
			return value
		end,
		section = function()
			return { choice = function() end }
		end,
	},
	json = {
		array = function()
			return {}
		end,
		encode = function(value)
			return value
		end,
	},
	net = {
		HOLD = "hold",
		intercept = function(definition)
			intercepts[definition.path] = definition
		end,
		request = function(options, fn)
			network = { url = options.url, callback = fn }
		end,
		respond = function(id, response)
			responded = { id = id, response = response }
		end,
		release = function() end,
	},
}
dofile(arg[1])
local search = intercepts["^/api/v%d+/gifs/search$"]
assert(
	search.request({ id = "1", url = "https://discord.com/api/v9/gifs/search?q=happy+cat&media_format=mp4" }) == "hold"
)
assert(network.url:find("api.tenor.com", 1, true) and network.url:find("q=happy%20cat", 1, true))
network.callback({
	status = 200,
	json = {
		results = {
			{
				id = "t1",
				itemurl = "https://tenor.com/t1",
				media = { { gif = { url = "https://g/1.gif" }, mp4 = { url = "https://g/1.mp4", dims = { 10, 20 } } } },
			},
		},
	},
})
assert(
	responded.id == "1"
		and responded.response.body[1].src == "https://g/1.mp4"
		and responded.response.body[1].width == 10
)
values.provider = "discord"
assert(search.request({ id = "2", url = "https://discord.com/api/v9/gifs/search?q=x" }) == nil)
print("gif-provider-switcher ok")
