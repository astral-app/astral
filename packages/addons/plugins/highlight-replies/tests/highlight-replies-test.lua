local callback

astral = {
	TextKind = { Message = "message" },
	Color = {
		Blurple = {
			with_alpha = function(_, alpha)
				return alpha * 0x1000000 + 0x5865F2
			end,
		},
	},
	ui = {
		style_text = function(value)
			callback = value
		end,
	},
}

dofile(arg[1])

assert(callback({ kind = "message", reply_to_me = true }) == 0x605865F2)
assert(callback({ kind = "message", reply_to_me = false }) == nil)

print("highlight-replies ok")
