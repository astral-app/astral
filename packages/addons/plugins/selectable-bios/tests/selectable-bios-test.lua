local patches = {}
astral = {
	log = function() end,
	ui = {
		patch = function(selector, fn)
			patches[selector.id] = fn
		end,
	},
}
dofile(arg[1])
assert(patches["profile-scroll"]({ id = "profile-scroll", key = "" }).selectable == true)
print("selectable-bios ok")
