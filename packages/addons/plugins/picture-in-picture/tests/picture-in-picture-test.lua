local build, press
local entered, toasted = true, nil
astral = {
	i18n = {
		t = function(key)
			return key
		end,
	},
	ui = {
		insert = function(selector, builder, on_press)
			assert(selector.id == "viewer-more" and selector.path == "^")
			build, press = builder, on_press
		end,
		picture_in_picture = function()
			return entered
		end,
	},
	toasts = {
		error = function(title)
			toasted = title
		end,
	},
}
dofile(arg[1])
local button = build({})
assert(button.action == "picture_in_picture" and button.children[1].icon)
press({}, "picture_in_picture")
assert(toasted == nil)
entered = false
press({}, "picture_in_picture")
assert(toasted == "START_FAILED")
print("picture-in-picture ok")
