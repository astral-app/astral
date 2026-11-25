local transform
local values = { explain = true, custom = "rp=Roleplay" }
astral = {
	log = function() end,
	settings = {
		get = function(key, fallback)
			if values[key] == nil then
				return fallback
			end
			return values[key]
		end,
		section = function()
			return { switch = function() end, text = function() end }
		end,
	},
	ui = {
		text_transform = function(fn)
			transform = fn
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
local out = transform({ { text = "that was funny /j and /rp. not /zzz or a/j", color = 1 } })
assert(out and out[1].text == "that was funny " and out[2].text == "/j joking" and out[2].bg)
assert(out[3].text == " and " and out[4].text == "/rp roleplay" and out[5].text == ". not /zzz or a/j")
assert(transform({ { text = "nothing here" } }) == nil)
values.explain = false
assert(transform({ { text = "hi /srs" } })[2].text == "/srs")
print("tone-indicators ok")
