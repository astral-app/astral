local declared = nil
local backdrops = {}

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
		settings_backdrop = function(options)
			table.insert(backdrops, options)
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
		and declared.options.default == 55
)
assert(declared.options.subtitle == "DIM_SUBTITLE")
assert(#backdrops == 1 and backdrops[1].dim == 55)

astral.on_setting("dim", 20)
assert(#backdrops == 2 and backdrops[2].dim == 20)

print("settings-theme ok")
