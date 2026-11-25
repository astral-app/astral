local patcher
local values = { scale = 115 }
astral = {
	log = function() end,
	settings = {
		get = function(key, fallback)
			return values[key] or fallback
		end,
		section = function()
			return { number = function() end }
		end,
	},
	ui = {
		patch = function(selector, fn)
			assert(selector.id == "message.content:")
			patcher = fn
		end,
	},
	discord = { permission = {} },
	i18n = {
		t = function(key)
			return key
		end,
	},
}
dofile(arg[1])
assert(math.abs(patcher({ id = "message.content:1" }).text_scale - 1.15) < 1e-9)
values.scale = 100
assert(patcher({ id = "message.content:1" }) == nil)
print("message-text-size ok")
