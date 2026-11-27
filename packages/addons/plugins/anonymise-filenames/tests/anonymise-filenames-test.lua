local intercepts = {}
local values = {}
local section = {}
section.switch = function() end
section.choice = function() end
section.text = function() end
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
			return section
		end,
		option = function(value)
			return value
		end,
		number = function() end,
	},
	time = {
		now_ms = function()
			return 1700000000000
		end,
	},
	net = {
		intercept = function(definition)
			intercepts[#intercepts + 1] = definition
		end,
	},
	json = {
		decode = function(text)
			return load("return " .. text)()
		end,
		encode = function(value)
			return value
		end,
	},
	i18n = {
		t = function(key)
			return key
		end,
	},
}
dofile(arg[1])
local json = { ["content-type"] = "application/json" }
local upload =
	intercepts[1].request({ headers = json, body = '{files = {{filename = "holiday photo.tar.gz", id = "1"}}}' })
local name = upload.body.files[1].filename
assert(#name == 7 + #".tar.gz" and name:sub(-7) == ".tar.gz", name)
local sent = intercepts[2].request({
	headers = json,
	body = '{attachments = {{filename = "holiday photo.tar.gz", id = "1", uploaded_filename = "x/holiday"}}}',
})
assert(sent.body.attachments[1].filename:sub(-7) == ".tar.gz")
values.method = "consistent"
values.name = "  "
local fixed = intercepts[1].request({ headers = json, body = '{files = {{filename = "a.png", id = "2"}}}' })
assert(fixed.body.files[1].filename == "image.png")
values.method = "timestamp"
local stamped = intercepts[1].request({ headers = json, body = '{files = {{filename = "b.jpeg", id = "3"}}}' })
assert(stamped.body.files[1].filename == "1700000000000.jpeg")
values.anonymise = false
assert(intercepts[1].request({ headers = json, body = '{files = {{filename = "c.png", id = "4"}}}' }) == nil)
print("anonymise-filenames ok")
