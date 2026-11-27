local build, press, gateway, watch, opened, selector
local timers = {}
local text = "hello **world**"
astral = {
	log = function() end,
	i18n = {
		t = function(key)
			return key
		end,
	},
	events = {
		on = function(_, fn)
			gateway = fn
		end,
	},
	discord = {
		me = function()
			return nil
		end,
		user = function(id)
			return { id = id, username = "me" }
		end,
	},
	time = {
		after = function(_, fn)
			timers[#timers + 1] = fn
		end,
	},
	ui = {
		composer = function(fn)
			watch = fn
		end,
		refresh = function() end,
		composer_text = function()
			return text
		end,
		sheet = function(spec)
			opened = spec
			return { close = function() end }
		end,
		insert = function(target, fn, on_press)
			selector, build, press = target, fn, on_press
		end,
	},
}
dofile(arg[1])

assert(selector.id == "composer.expressions" and selector.path == "^")
assert(build() == nil)
watch("hello")
local button = build()
assert(button and button.width == 32 and button.action == "preview")
watch("   ")
assert(build() == nil)
gateway({ type = "READY", data = { user = { id = "1" } } })
press()
assert(opened and opened.title == "MESSAGE_PREVIEW")
local row = opened.items[1]
assert(row.items[1].round and row.items[2].items[2].text == "hello **world**")
text = "  "
opened = nil
press()
assert(opened == nil)
print("preview-message ok")
