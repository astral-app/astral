local declared, patcher, selector = nil, nil, nil
local refreshes = 0
local background = { angle = 90, colors = { 0xFF204060, 0xFF000000 } }

astral = {
	log = function() end,
	i18n = {
		t = function(key)
			return key
		end,
	},
	settings = {
		number = function(key, title, options)
			declared = { key = key, title = title, options = options }
			return options.default
		end,
		switch = function() end,
		choice = function() end,
	},
	ui = {
		decorate = function() end,
		relabel = function() end,
		patch = function(id, _, fn)
			selector, patcher = id, fn
		end,
		app_background = function()
			return background
		end,
		refresh = function()
			refreshes = refreshes + 1
		end,
	},
	discord = { permission = {} },
}

dofile(arg[2])
dofile(arg[1])

assert(declared.key == "dim" and declared.title == "DIM")
assert(
	declared.options.min == 0
		and declared.options.max == 90
		and declared.options.step == 5
		and declared.options.default == 0
)
assert(declared.options.subtitle == "DIM_SUBTITLE")
assert(selector == "settings-phone")

local patch = patcher({ id = "settings-phone" })
assert(patch.background == 0 and patch.see_through == true)
assert(patch.gradient.angle == 90 and patch.gradient.colors[1] == 0xFF204060)

astral.on_setting("dim", 50)
assert(refreshes == 1)
patch = patcher({ id = "settings-phone" })
assert(patch.gradient.colors[1] == 0xFF102030 and patch.gradient.colors[2] == 0xFF000000)

background = nil
assert(patcher({ id = "settings-phone" }) == nil)

print("settings-theme ok")
