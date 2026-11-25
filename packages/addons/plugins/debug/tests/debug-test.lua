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
				device_name = "iPhone SE (2nd generation)",
				discord_channel = "canary",
				compatibility = "ready",
				commit = "4bb262921",
				image = "astral.dylib",
				framework = "ElleKit 1.1.3",
				jailbreak = "Dopamine 2.4",
				source = "App Store",
				signer = "sha256 f02ef369d657b82a · Apple iPhone OS Application Signing",
				binary = "encrypted · min 16.0 · SDK 27.1 · 107 dylibs · UUID 2F737236",
				markers = { "Developer Mode on" },
				https = { issuer = "GTS Root R4" },
				architecture = "arm64",
				phoenix = "bc893919-ca31-3575-b708-ee4d03486c1f",
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
	json = {
		encode = function(value)
			return "discord=" .. tostring(value.discord) .. " failed=" .. table.concat(value.plugins_failed or {}, ",")
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
assert(reply.send.file.description == "discord=357.0 failed=Translate")
assert(rendered.width == 432 and rendered.scale == 3 and #rendered.items > 30)
local texts = {}
for _, item in ipairs(rendered.items) do
	if item.type == "text" then
		texts[#texts + 1] = item.text
		assert(item.x + utf8.len(item.text) * 13 * 0.6 <= 432 - 13, "text runs past the card: " .. item.text)
	end
end
local joined = table.concat(texts, "|")
for _, expected in ipairs({
	"astral /debug",
	" @4bb262921",
	"supported",
	"iPhone SE (2nd generation)",
	"Translate",
	"● dopamine 2.4",
	"2026-11-25 05:10 utc",
}) do
	assert(joined:find(expected, 1, true), "missing " .. expected)
end
assert(not joined:find("107 dylibs", 1, true))
print("debug ok")
