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
				jailbreak = "Dopamine 2.4",
				source = "App Store",
				signer = "sha256 f02ef369d657b82a · Apple iPhone OS Application Signing",
				binary = "encrypted · min 16.0 · SDK 27.1 · 107 dylibs · UUID 2F737236",
				markers = { "120 Hz", "Developer Mode on" },
				https = { issuer = "GTS Root R4" },
				architecture = "arm64",
				phoenix = "bc893919-ca31-3575-b708-ee4d03486c1f",
				reports = 3,
				crashes_24h = 2,
				last_report = "Crash: EXC_BAD_ACCESS",
				os_build = "23B85",
				framework = "ElleKit 1.1.3-2",
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
	if item.type == astral.ImageItemType.Text then
		texts[#texts + 1] = item.text
		local glyph = (item.mono and 0.6 or 0.56) * item.size
		local right = item.width and (item.x + item.width) or (item.x + utf8.len(item.text) * glyph)
		assert(right <= 432 - 8, "text runs past the card: " .. item.text)
		assert(
			not item.width or item.align == astral.TextAlign.Center or utf8.len(item.text) * glyph <= item.width + 1,
			"text is cut off: " .. item.text
		)
	end
end
local joined = table.concat(texts, "|")
for _, expected in ipairs({
	"@4bb262921",
	"supported",
	" alpha · 124376",
	"iPhone SE (2nd generation)",
	"iOS 26.1 (23B85)",
	" · ElleKit 1.1.3",
	"Translate",
	"2 in 24h",
	"2/2 plugins",
	"dopamine 2.4  ",
	"GTS Root R4",
	"Developer Mode on",
	"bc893919",
	"2026-11-25 05:10 utc",
}) do
	assert(joined:find(expected, 1, true), "missing " .. expected)
end
for _, absent in ipairs({ "107 dylibs", "120 Hz", "1.1.3-2", "package", "Debug 0.0.1", "Last problem" }) do
	assert(not joined:find(absent, 1, true), "should not show " .. absent)
end
local last_line = 0
for _, item in ipairs(rendered.items) do
	if item.type == astral.ImageItemType.Text and item.y > last_line then
		last_line = item.y
	end
end
for _, item in ipairs(rendered.items) do
	if item.type == astral.ImageItemType.Text and item.y == last_line and item.text ~= "" then
		assert(item.x >= 12 + 64, "text sits under the ALT badge: " .. item.text)
	end
end
print("debug ok")
