local typing = nil
local users = { u1 = { id = "u1", avatar = "abc" } }

astral = {
	log = function() end,
	i18n = {
		t = function(key)
			return key
		end,
	},
	settings = { switch = function() end, choice = function() end, number = function() end },
	ui = {
		decorate = function() end,
		relabel = function() end,
		style_text = function(fn)
			typing = function(node)
				node.kind = "typing"
				return fn(node)
			end
		end,
	},
	discord = {
		permission = {},
		user = function(id)
			return users[id]
		end,
	},
}

dofile(arg[2])
dofile(arg[1])

assert(typing({ text = "Rae", user_id = "u1" }).image == "https://cdn.discordapp.com/avatars/u1/abc.webp?size=48")
assert(
	typing({ text = "Kode", user_id = "263689920210534400" }).image
		== "https://cdn.discordapp.com/embed/avatars/" .. ((263689920210534400 >> 22) % 6) .. ".png"
)
assert(typing({ text = "Nobody" }) == nil)

print("typing-avatars ok")
