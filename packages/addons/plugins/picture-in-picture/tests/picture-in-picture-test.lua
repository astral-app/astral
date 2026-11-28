local build, press
local entered, toasted = true, nil
local patches, events, refreshes = {}, {}, 0
astral = {
	i18n = {
		t = function(key)
			return key
		end,
	},
	Event = {
		PictureInPictureStart = "picture_in_picture.start",
		PictureInPictureStop = "picture_in_picture.stop",
	},
	ui = {
		insert = function(selector, builder, on_press)
			assert(selector.id == "viewer-more" and selector.path == "^")
			build, press = builder, on_press
		end,
		patch = function(selector, patcher)
			patches[selector.id] = patcher
		end,
		refresh = function()
			refreshes = refreshes + 1
		end,
		picture_in_picture = function()
			return entered
		end,
	},
	events = {
		on = function(event, callback)
			events[event] = callback
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
assert(patches["viewer-header"]() == nil)
events["picture_in_picture.start"]()
assert(patches["viewer-header"]().hidden)
assert(patches["viewer-seek-bar"]().hidden)
assert(patches["viewer-message"]().hidden)
assert(refreshes == 1)
events["picture_in_picture.stop"]()
assert(patches["viewer-header"]() == nil)
assert(refreshes == 2)
print("picture-in-picture ok")
