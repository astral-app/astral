local command
local rendered
astral = {
	log = function() end,
	settings = {},
	ui = {},
	discord = { permission = {} },
	i18n = {
		t = function(key)
			return key
		end,
	},
	commands = {
		register = function(definition)
			command = definition
		end,
	},
	diagnostics = {
		environment = function()
			return {
				astral = "0.0.1",
				discord = "357.0 (124376)",
				platform = "ios",
				system = "iOS 26.1",
				install_mode = "package",
				device = "iPhone",
				architecture = "arm64",
				phoenix = "",
				reports = 0,
				plugin_list = {
					{ id = "astral.debug", name = "Debug", version = "0.0.1", enabled = true, running = true },
					{ id = "astral.translate", name = "Translate", version = "0.0.3", enabled = true, failed = true },
				},
			}
		end,
	},
	image = {
		render = function(spec)
			rendered = spec
			return "PNG"
		end,
	},
	time = {
		iso = function()
			return "2026-11-25 05:10 UTC"
		end,
	},
}
dofile(arg[2])
dofile(arg[1])

assert(command and command.name == "debug")
local reply = command.execute({ message = "help" })
assert(reply.send.content == "help" and reply.send.file.data == "PNG" and reply.send.file.name == "astral-debug.png")
assert(reply.send.file.description:find("Discord: 357.0", 1, true))
assert(reply.send.file.description:find("astral.translate 0.0.3 failed", 1, true))
assert(rendered.width == 720 and rendered.height > 300 and #rendered.items > 20)
local issues = false
for _, item in ipairs(rendered.items) do
	issues = issues or item.text == "Issues found"
end
assert(issues)
print("debug ok")
