local WIDTH = 432
local SCALE = 3
local PADDING = 14
local TITLE_HEIGHT = 28
local FONT_SIZE = 13
local LINE_HEIGHT = 22
local CHAR_WIDTH = FONT_SIZE * 0.6
local LABEL_WIDTH = 9
local BADGE_CLEARANCE = 66
local FOOTER_HEIGHT = 52
local MAX_ALT_TEXT = 1024
local NOISE_BINARY_PART = { "^%d+ dylibs$", "^%d+ injected$", "^UUID %S+$" }
local ALT_DROP_ORDER =
	{ "plugins_failed", "markers", "https", "binary", "signer", "install", "source", "framework", "jailbreak" }
local CHANNELS = { canary = "alpha", ptb = "stable", stable = "stable", beta = "beta" }

local COLORS = {
	background = 0xFF111214,
	border = 0xFF2B2D31,
	title_bar = 0xFF1E1F22,
	label = 0xFF949BA4,
	value = 0xFFF2F3F5,
	accent = 0xFFA78BFA,
	dim = 0xFF949BA4,
	good = 0xFF22C55E,
	warn = 0xFFEAB308,
	bad = 0xFFEF4444,
}

local function value(text)
	return { text = text, color = COLORS.value }
end

local function dim(text)
	return { text = text, color = COLORS.dim }
end

local function colored(text, color)
	return { text = text, color = color }
end

local function present(text)
	return type(text) == "string" and text ~= ""
end

local function joined(parts)
	local kept = {}
	for _, part in ipairs(parts) do
		if present(part) then
			kept[#kept + 1] = part
		end
	end
	return table.concat(kept, " · ")
end

local function binary_text(binary)
	local kept = {}
	for part in (binary .. " · "):gmatch("(.-) · ") do
		local noise = false
		for _, pattern in ipairs(NOISE_BINARY_PART) do
			noise = noise or part:match(pattern) ~= nil
		end
		if not noise then
			kept[#kept + 1] = part
		end
	end
	return table.concat(kept, " · ")
end

local function discord_parts(environment)
	local version, build = tostring(environment.discord or "?"):match("^(%S+) %((%S+)%)$")
	local channel = CHANNELS[tostring(environment.discord_channel or ""):lower()] or environment.discord_channel
	return version or tostring(environment.discord or "?"), build, channel
end

local function support_spans(environment)
	local compatibility = environment.compatibility
	if not present(compatibility) then
		return {}
	end
	if compatibility == "ready" then
		return { dim(" · "), colored("supported", COLORS.good) }
	end
	return { dim(" · "), colored(compatibility, COLORS.warn) }
end

local function https_spans(https)
	if type(https) ~= "table" then
		return nil
	end
	if https.intercepted then
		return { colored("MITM · " .. (https.issuer or "unknown CA"), COLORS.warn) }
	end
	if present(https.failure) then
		return { dim("unchecked · " .. https.failure) }
	end
	return { value("direct"), dim(present(https.issuer) and (" · " .. https.issuer) or "") }
end

local function plugin_counts(environment)
	local enabled, installed, failed = 0, 0, {}
	for _, plugin in ipairs(environment.plugin_list or {}) do
		installed = installed + 1
		if plugin.enabled then
			enabled = enabled + 1
		end
		if plugin.failed then
			failed[#failed + 1] = plugin.name or plugin.id
		end
	end
	return enabled, installed, failed
end

local function append(rows, label, spans)
	if spans and #spans > 0 then
		rows[#rows + 1] = { label, spans }
	end
end

local function rows(environment)
	local list = {}
	local version, build, channel = discord_parts(environment)
	append(list, "astral", {
		value(environment.astral or "?"),
		present(environment.commit) and colored(" @" .. environment.commit, COLORS.accent) or nil,
	})
	append(list, "loader", {
		value(environment.image or "astral"),
		dim(" " .. joined({ environment.install_mode, environment.framework })),
	})
	local discord = { value(version), dim(" " .. joined({ channel, build })) }
	for _, span in ipairs(support_spans(environment)) do
		discord[#discord + 1] = span
	end
	append(list, "discord", discord)
	append(list, "source", present(environment.source) and { value(environment.source) } or nil)
	append(list, "device", { value(environment.device_name or environment.device or "?") })
	append(list, "system", {
		value(environment.system or "?"),
		present(environment.os_beta) and colored(" · beta " .. environment.os_beta, COLORS.warn) or nil,
	})
	append(list, "install", present(environment.install) and { value((environment.install:gsub("`", ""))) } or nil)
	append(list, "signer", present(environment.signer) and { value(environment.signer) } or nil)
	append(list, "binary", present(environment.binary) and { value(binary_text(environment.binary)) } or nil)
	local markers = type(environment.markers) == "table" and table.concat(environment.markers, " · ") or ""
	append(list, "markers", present(markers) and { dim(markers) } or nil)
	append(list, "https", https_spans(environment.https))
	append(
		list,
		"phoenix",
		present(environment.phoenix)
				and { value(environment.phoenix:sub(1, 8)), dim(" · " .. (environment.engine or "Phoenix")) }
			or nil
	)
	local _, _, failed = plugin_counts(environment)
	append(list, "failed", #failed > 0 and { colored(table.concat(failed, " · "), COLORS.bad) } or nil)
	append(list, "crash", present(environment.last_report) and { colored(environment.last_report, COLORS.warn) } or nil)
	return list
end

local function ordered_spans(spans)
	local kept = {}
	for index = 1, 8 do
		local span = spans[index]
		if span and present(span.text) then
			kept[#kept + 1] = span
		end
	end
	return kept
end

local function wrapped(spans, room)
	local lines, line, used = {}, {}, 0
	local function flush()
		lines[#lines + 1] = line
		line, used = {}, 0
	end
	for _, span in ipairs(spans) do
		local text = span.text
		while #text > 0 do
			local left = room - used
			if utf8.len(text) <= left then
				line[#line + 1] = { text = text, color = span.color }
				used = used + utf8.len(text)
				text = ""
			else
				local cut = utf8.offset(text, left + 1) or (#text + 1)
				local head = text:sub(1, cut - 1)
				local space = head:match(".*() ")
				if space and space > 1 then
					head = text:sub(1, space - 1)
				end
				if #head == 0 and used > 0 then
					flush()
				else
					line[#line + 1] = { text = head, color = span.color }
					text = text:sub(#head + 1):gsub("^ ", "")
					flush()
				end
			end
		end
	end
	if #line > 0 then
		flush()
	end
	return lines
end

local function text_runs(items, spans, x, top)
	for _, span in ipairs(spans) do
		items[#items + 1] = {
			type = "text",
			x = x,
			y = top,
			text = span.text,
			size = FONT_SIZE,
			mono = true,
			color = span.color,
		}
		x = x + utf8.len(span.text) * CHAR_WIDTH
	end
end

local function state_spans(environment)
	local spans = {}
	if present(environment.jailbreak) then
		spans[#spans + 1] = colored("● " .. environment.jailbreak:lower(), COLORS.dim)
	end
	local _, _, failed = plugin_counts(environment)
	spans[#spans + 1] = #failed > 0 and colored("● " .. #failed .. " failing", COLORS.bad)
		or colored("● healthy", COLORS.good)
	for index = 2, #spans do
		spans[index].text = "  " .. spans[index].text
	end
	return spans
end

local function draw_card(environment, stamp)
	local room = math.floor((WIDTH - PADDING * 2) / CHAR_WIDTH)
	local value_room = room - LABEL_WIDTH
	local value_x = PADDING + LABEL_WIDTH * CHAR_WIDTH
	local lines = {}
	for _, row in ipairs(rows(environment)) do
		for index, line in ipairs(wrapped(ordered_spans(row[2]), value_room)) do
			lines[#lines + 1] = { label = index == 1 and row[1] or "", line = line }
		end
	end
	local footer_top = TITLE_HEIGHT + PADDING + #lines * LINE_HEIGHT + PADDING / 2
	local height = footer_top + FOOTER_HEIGHT
	local text_offset = FONT_SIZE * 0.95
	local items = {
		{ type = "rect", x = 0, y = 0, w = WIDTH, h = height, radius = 8, color = COLORS.background },
		{ type = "rect", x = 0, y = 0, w = WIDTH, h = TITLE_HEIGHT, radius = 8, color = COLORS.title_bar },
		{ type = "rect", x = 0, y = TITLE_HEIGHT - 8, w = WIDTH, h = 8, color = COLORS.title_bar },
		{ type = "circle", x = 16, y = TITLE_HEIGHT / 2, r = 4.5, color = COLORS.bad },
		{ type = "circle", x = 30, y = TITLE_HEIGHT / 2, r = 4.5, color = COLORS.warn },
		{ type = "circle", x = 44, y = TITLE_HEIGHT / 2, r = 4.5, color = COLORS.good },
	}
	local title_top = TITLE_HEIGHT / 2 + 4 - text_offset
	text_runs(items, { dim("astral /debug") }, 60, title_top)
	text_runs(items, { dim(stamp) }, WIDTH - PADDING - utf8.len(stamp) * CHAR_WIDTH, title_top)
	for index, entry in ipairs(lines) do
		local top = TITLE_HEIGHT + PADDING + (index - 1) * LINE_HEIGHT + FONT_SIZE - text_offset
		text_runs(items, { colored(entry.label, COLORS.label) }, PADDING, top)
		text_runs(items, entry.line, value_x, top)
	end
	items[#items + 1] =
		{ type = "rect", x = 0, y = footer_top, w = WIDTH, h = FOOTER_HEIGHT, radius = 8, color = COLORS.title_bar }
	items[#items + 1] = { type = "rect", x = 0, y = footer_top, w = WIDTH, h = 8, color = COLORS.title_bar }
	local footer_text_top = footer_top + FOOTER_HEIGHT / 2 + 4 - text_offset
	text_runs(items, state_spans(environment), BADGE_CLEARANCE, footer_text_top)
	local enabled, installed = plugin_counts(environment)
	local summary = string.format("%d/%d plugins", enabled, installed)
	text_runs(items, { dim(summary) }, WIDTH - PADDING - #summary * CHAR_WIDTH, footer_text_top)
	items[#items + 1] = {
		type = "rect",
		x = 0.5,
		y = 0.5,
		w = WIDTH - 1,
		h = height - 1,
		radius = 8,
		border = COLORS.border,
		border_width = 1,
	}
	return { width = WIDTH, height = height, scale = SCALE, items = items }
end

local function alt_payload(environment)
	local version, build, channel = discord_parts(environment)
	local enabled, installed, failed = plugin_counts(environment)
	return {
		astral = environment.astral,
		commit = environment.commit,
		loader = environment.image,
		framework = environment.framework,
		jailbreak = environment.jailbreak,
		discord = version,
		channel = channel,
		build = build,
		support = environment.compatibility,
		source = environment.source,
		install = present(environment.install) and environment.install:gsub("`", "") or nil,
		device = environment.device_name or environment.device,
		system = environment.system,
		signer = environment.signer,
		binary = environment.binary,
		markers = environment.markers,
		https = environment.https,
		phoenix = environment.phoenix,
		plugins = string.format("%d/%d", enabled, installed),
		plugins_failed = #failed > 0 and failed or nil,
		crash = environment.last_report,
	}
end

local function alt_text(environment)
	local payload = alt_payload(environment)
	local text = astral.json.encode(payload)
	for _, key in ipairs(ALT_DROP_ORDER) do
		if #text <= MAX_ALT_TEXT then
			break
		end
		payload[key] = nil
		text = astral.json.encode(payload)
	end
	return text:sub(1, MAX_ALT_TEXT)
end

local function stamp()
	return (astral.time.iso():gsub("UTC$", "utc"))
end

astral.commands.register({
	name = "debug",
	description = "Posts astral's debug card for support",
	options = { { name = "message", description = "Add a note above the card", type = "string" } },
	execute = function(args)
		local environment = astral.diagnostics.environment()
		local png = astral.image.render(draw_card(environment, stamp()))
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
