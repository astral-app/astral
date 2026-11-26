local enums = dofile(arg[1])

local function with_enums(value)
	if rawget(value, "enums") == nil then
		rawset(value, "enums", enums)
	end
	for name, members in pairs(enums) do
		if rawget(value, name) == nil then
			rawset(value, name, members)
		end
	end
end

local current_astral = {}
with_enums(current_astral)

setmetatable(_G, {
	__index = function(_, key)
		if key == "astral" then
			return current_astral
		end
	end,
	__newindex = function(globals, key, value)
		if key ~= "astral" then
			rawset(globals, key, value)
			return
		end
		if type(value) == "table" then
			with_enums(value)
		end
		current_astral = value
	end,
})

local test = arg[2]
arg = { [0] = test, arg[3], arg[4] }
dofile(test)
