local WIDTH = 720
local PADDING = 28
local TILE_GAP = 12
local TILE_HEIGHT = 72

local COLORS = {
	card = 0xFF1E1F22,
	tile = 0xFF2B2D31,
	text = 0xFFF2F3F5,
	muted = 0xFF949BA4,
	accent = 0xFF5865F2,
	healthy = 0xFF23A55A,
	problem = 0xFFDA373C,
	idle = 0xFF80848E,
}

local function plugin_state(plugin)
	if plugin.failed then
		return COLORS.problem, "failed"
	elseif plugin.running then
		return COLORS.healthy, "running"
	end
	return COLORS.idle, "off"
end

local function facts(environment)
	local running, failed, installed = 0, 0, 0
	for _, plugin in ipairs(environment.plugin_list or {}) do
		installed = installed + 1
		if plugin.failed then
			failed = failed + 1
		elseif plugin.running then
			running = running + 1
		end
	end
	return {
		{ "Discord", environment.discord or "unknown" },
		{ "astral", environment.astral or "unknown" },
		{
			"Platform",
			(environment.system or environment.platform or "unknown") .. " · " .. (environment.install_mode or "?"),
		},
		{ "Device", (environment.device or "?") .. " · " .. (environment.architecture or environment.abi or "?") },
		{ "Plugins", string.format("%d running · %d failed · %d installed", running, failed, installed) },
		{ "Phoenix", environment.phoenix ~= "" and environment.phoenix or "linked" },
	},
		failed
end

local function add(items, item)
	items[#items + 1] = item
end

local function draw_tile(items, x, y, width, label, value)
	add(items, { type = "rect", x = x, y = y, w = width, h = TILE_HEIGHT, radius = 12, color = COLORS.tile })
	add(
		items,
		{ type = "text", x = x + 16, y = y + 12, width = width - 32, text = label, size = 13, color = COLORS.muted }
	)
	add(items, {
		type = "text",
		x = x + 16,
		y = y + 34,
		width = width - 32,
		text = value,
		size = 17,
		weight = "semibold",
		color = COLORS.text,
	})
end

local function draw_card(environment, now)
	local tiles, failed = facts(environment)
	local plugins = environment.plugin_list or {}
	local columns = 2
	local tile_width = (WIDTH - PADDING * 2 - TILE_GAP) / columns
	local tile_rows = math.ceil(#tiles / columns)
	local plugin_rows = math.ceil(#plugins / columns)
	local plugins_top = 112 + tile_rows * (TILE_HEIGHT + TILE_GAP) + 20
	local height = plugins_top + 30 + plugin_rows * 26 + 64
	local healthy = failed == 0 and (environment.reports or 0) == 0
	local items = {}
	add(items, { type = "rect", x = 0, y = 0, w = WIDTH, h = height, radius = 24, color = COLORS.card })
	add(items, {
		type = "text",
		x = PADDING,
		y = PADDING - 2,
		text = "astral",
		size = 30,
		weight = "bold",
		color = COLORS.text,
	})
	local version = environment.astral or "?"
	add(items, {
		type = "rect",
		x = PADDING + 104,
		y = PADDING + 6,
		w = 22 + #version * 9,
		h = 26,
		radius = 13,
		color = COLORS.accent,
	})
	add(items, {
		type = "text",
		x = PADDING + 104,
		y = PADDING + 10,
		width = 22 + #version * 9,
		text = version,
		size = 14,
		weight = "semibold",
		align = "center",
		color = COLORS.text,
	})
	local status = healthy and "Healthy" or "Issues found"
	local pill = 40 + #status * 9
	add(items, {
		type = "rect",
		x = WIDTH - PADDING - pill,
		y = PADDING + 4,
		w = pill,
		h = 30,
		radius = 15,
		color = healthy and COLORS.healthy or COLORS.problem,
	})
	add(items, {
		type = "text",
		x = WIDTH - PADDING - pill,
		y = PADDING + 10,
		width = pill,
		text = status,
		size = 15,
		weight = "bold",
		align = "center",
		color = COLORS.text,
	})
	add(
		items,
		{ type = "text", x = PADDING, y = PADDING + 44, text = now, size = 14, mono = true, color = COLORS.muted }
	)
	for index, tile in ipairs(tiles) do
		local column = (index - 1) % columns
		local row = (index - 1) // columns
		draw_tile(
			items,
			PADDING + column * (tile_width + TILE_GAP),
			112 + row * (TILE_HEIGHT + TILE_GAP),
			tile_width,
			tile[1],
			tile[2]
		)
	end
	add(items, {
		type = "text",
		x = PADDING,
		y = plugins_top,
		text = "Plugins",
		size = 15,
		weight = "semibold",
		color = COLORS.muted,
	})
	for index, plugin in ipairs(plugins) do
		local column = (index - 1) % columns
		local row = (index - 1) // columns
		local x = PADDING + column * (tile_width + TILE_GAP)
		local y = plugins_top + 30 + row * 26
		local color, state = plugin_state(plugin)
		add(items, { type = "circle", x = x + 6, y = y + 10, r = 5, color = color })
		add(items, {
			type = "text",
			x = x + 20,
			y = y,
			width = tile_width - 20,
			text = (plugin.name or plugin.id)
				.. " "
				.. (plugin.version or "")
				.. (state == "running" and "" or " · " .. state),
			size = 15,
			color = plugin.enabled and COLORS.text or COLORS.muted,
		})
	end
	local footer = environment.last_report and ("Last problem: " .. environment.last_report)
		or "No crashes or plugin errors recorded"
	add(items, {
		type = "text",
		x = PADDING + 64,
		y = height - 40,
		width = WIDTH - PADDING * 2 - 64,
		text = footer,
		size = 14,
		color = COLORS.muted,
	})
	return { width = WIDTH, height = height, scale = 2, items = items }
end

local function alt_text(environment)
	local lines = {}
	for _, fact in ipairs((facts(environment))) do
		lines[#lines + 1] = fact[1] .. ": " .. fact[2]
	end
	for _, plugin in ipairs(environment.plugin_list or {}) do
		local _, state = plugin_state(plugin)
		lines[#lines + 1] = (plugin.id or "?") .. " " .. (plugin.version or "") .. " " .. state
	end
	return table.concat(lines, "\n")
end

astral.commands.register({
	name = "debug",
	description = "Posts astral's status card for support",
	options = { { name = "message", description = "Add a note above the card", type = "string" } },
	execute = function(args)
		local environment = astral.diagnostics.environment()
		local png = astral.image.render(draw_card(environment, astral.time.iso()))
		if not png then
			return "Couldn't draw the debug card on this device."
		end
		return {
			send = {
				content = args.message or "",
				file = { name = "astral-debug.png", data = png, description = alt_text(environment) },
			},
		}
	end,
})
