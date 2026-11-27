local rewrites = {}
astral = {
	log = function() end,
	ui = {
		image_rewrite = function(prefix, fn)
			rewrites[prefix] = fn
		end,
	},
	discord = {
		emoji = function(id)
			if id == "5" then
				return { id = id, name = "wave", animated = true }
			end
			if id == "6" then
				return { id = id, name = "github", animated = false }
			end
			return nil
		end,
	},
}
dofile(arg[1])
local avatars = rewrites["https://cdn.discordapp.com/avatars/"]
assert(
	avatars("https://cdn.discordapp.com/avatars/1/a_abc123.webp?size=44")
		== "https://cdn.discordapp.com/avatars/1/a_abc123.webp?size=44&animated=true"
)
assert(avatars("https://cdn.discordapp.com/avatars/1/abc123.webp?size=44") == nil)
local emojis = rewrites["https://cdn.discordapp.com/emojis/"]
assert(emojis("https://cdn.discordapp.com/emojis/5.webp") == "https://cdn.discordapp.com/emojis/5.webp?animated=true")
assert(emojis("https://cdn.discordapp.com/emojis/5.webp?animated=true") == nil)
assert(emojis("https://cdn.discordapp.com/emojis/6.webp?size=48") == nil)
assert(emojis("https://cdn.discordapp.com/emojis/7.webp?size=48") == nil)
print("always-animate ok")
