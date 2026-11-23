local callback

astral = {
	Color = {
		Blurple = {
			with_alpha = function(_, alpha)
				return alpha * 0x1000000 + 0x5865F2
			end,
		},
	},
	ui = {
		message_color = function(value)
			callback = value
		end,
	},
}

dofile(arg[1])

assert(callback({ reply_to_me = true }) == 0x605865F2)
assert(callback({ reply_to_me = false }) == nil)

print("highlight-replies ok")
