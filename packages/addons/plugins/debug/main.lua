local WIDTH = 480
local SCALE = 3
local PADDING = 10
local TILE_GAP = 5
local TILE_PADDING = 7
local LABEL_SIZE = 10
local VALUE_SIZE = 13
local DETAIL_SIZE = 11
local HEADER_HEIGHT = 36
local COLUMNS = 3
local GLYPH_WIDTH = 0.56
local VALUE_GLYPH_WIDTH = 0.6
local MAX_ALT_TEXT = 1024
local NOISE_BINARY_PART = { "^%d+ dylibs$", "^%d+ injected$", "^UUID %S+$" }
local ALT_DROP_ORDER =
	{ "plugins_failed", "markers", "https", "binary", "signer", "install", "source", "framework", "jailbreak" }
local CHANNELS = { canary = "alpha", alpha = "alpha", ptb = "stable", stable = "stable", beta = "beta" }

local COLORS = {
	card = 0xFF1E1F22,
	tile = 0xFF2B2D31,
	label = 0xFF949BA4,
	value = 0xFFF2F3F5,
	dim = 0xFF949BA4,
	accent = 0xFF5865F2,
	commit = 0xFFA78BFA,
	good = 0xFF23A55A,
	warn = 0xFFF0B232,
	bad = 0xFFDA373C,
	idle = 0xFF80848E,
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
	local text = tostring(environment.discord or "?")
	local version, build = text:match("^(%S+) %((%S+)%)$")
	local named_channel
	if not version then
		version, named_channel, build = text:match("^(%S+) %- (%a+) · (%d+)$")
	end
	local raw_channel = tostring(environment.discord_channel or named_channel or ""):lower()
	local channel = CHANNELS[raw_channel] or (raw_channel ~= "" and raw_channel or nil)
	return version or text, build, channel
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
	append(list, "injection", {
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
	append(list, "markers", present(markers) and { value(markers) } or nil)
	append(list, "https", https_spans(environment.https))
	append(list, "phoenix", present(environment.phoenix) and {
		value(environment.phoenix:sub(1, 8)),
		present(environment.engine) and environment.engine ~= "Phoenix" and dim(" · " .. environment.engine) or nil,
	} or nil)
	local _, _, failed = plugin_counts(environment)
	append(list, "failed", #failed > 0 and { colored(table.concat(failed, " · "), COLORS.bad) } or nil)
	append(list, "crash", present(environment.last_report) and { colored(environment.last_report, COLORS.warn) } or nil)
	return list
end

local function trimmed(text)
	return (text:gsub("^[%s·]+", ""):gsub("[%s·]+$", ""))
end

local function detail_groups(spans)
	local groups = {}
	for index = 2, #spans do
		local span = spans[index]
		local color = span.color == COLORS.value and COLORS.dim or span.color
		local last = groups[#groups]
		if last and last.color == color then
			last.text = last.text .. span.text
		else
			groups[#groups + 1] = { text = span.text, color = color }
		end
	end
	local kept = {}
	for _, group in ipairs(groups) do
		group.text = trimmed(group.text)
		if group.text ~= "" then
			kept[#kept + 1] = group
		end
	end
	return kept
end

local function tile_of(row)
	local spans = {}
	for index = 1, 8 do
		local span = row[2][index]
		if span and present(span.text) then
			spans[#spans + 1] = span
		end
	end
	local head = spans[1]
	return {
		label = row[1],
		value = head and head.text or "",
		value_color = head and head.color or COLORS.value,
		details = detail_groups(spans),
	}
end

local function wrap(text, size, width)
	local glyph = size == VALUE_SIZE and VALUE_GLYPH_WIDTH or GLYPH_WIDTH
	local room = math.max(1, math.floor(width / (size * glyph)))
	local lines, line = {}, ""
	for found in text:gmatch("%S+") do
		local word = found
		while utf8.len(word) > room do
			if line ~= "" then
				lines[#lines + 1] = line
				line = ""
			end
			local cut = utf8.offset(word, room + 1)
			lines[#lines + 1] = word:sub(1, cut - 1)
			word = word:sub(cut)
		end
		local candidate = line == "" and word or (line .. " " .. word)
		if utf8.len(candidate) <= room then
			line = candidate
		else
			lines[#lines + 1] = line
			line = word
		end
	end
	if line ~= "" then
		lines[#lines + 1] = line
	end
	return lines
end

local function line_height(size)
	return math.ceil(size * 1.3)
end

local function text_width(text, size)
	return utf8.len(text) * size * (size == VALUE_SIZE and VALUE_GLYPH_WIDTH or GLYPH_WIDTH)
end

local function inline_detail(tile, inner)
	local detail = tile.details[#tile.details]
	if not detail or detail.color == COLORS.dim then
		return nil
	end
	local room = inner - text_width(tile.value, VALUE_SIZE) - 8
	return text_width(detail.text, DETAIL_SIZE) <= room and detail or nil
end

local function arrange(tile, width)
	local inner = width - TILE_PADDING * 2
	tile.inline = inline_detail(tile, inner)
	tile.value_lines = wrap(tile.value, VALUE_SIZE, inner)
	tile.detail_lines = {}
	for _, detail in ipairs(tile.details) do
		if detail ~= tile.inline then
			for _, line in ipairs(wrap(detail.text, DETAIL_SIZE, inner)) do
				tile.detail_lines[#tile.detail_lines + 1] = { text = line, color = detail.color }
			end
		end
	end
	return TILE_PADDING * 2
		+ line_height(LABEL_SIZE)
		+ #tile.value_lines * line_height(VALUE_SIZE)
		+ #tile.detail_lines * line_height(DETAIL_SIZE)
end

local function third_width()
	return (WIDTH - PADDING * 2 - TILE_GAP * (COLUMNS - 1)) / COLUMNS
end

local function half_width()
	return (WIDTH - PADDING * 2 - TILE_GAP) / 2
end

local function single_lines(tile, width)
	arrange(tile, width)
	local expected = 0
	for _, detail in ipairs(tile.details) do
		if detail ~= tile.inline then
			expected = expected + 1
		end
	end
	return #tile.value_lines == 1 and #tile.detail_lines == expected
end

local function grouped(tiles)
	local groups = { thirds = {}, halves = {}, fulls = {} }
	for _, tile in ipairs(tiles) do
		if single_lines(tile, third_width()) then
			groups.thirds[#groups.thirds + 1] = tile
		elseif single_lines(tile, half_width()) then
			groups.halves[#groups.halves + 1] = tile
		else
			groups.fulls[#groups.fulls + 1] = tile
		end
	end
	return groups
end

local function short_rows(thirds)
	local packed, count = {}, #thirds
	local tail = count % COLUMNS == 1 and 1 or math.min(2, count)
	local rest = count - tail
	local index = 1
	while rest > 0 do
		local take = (rest == 4) and 2 or math.min(COLUMNS, rest)
		packed[#packed + 1] = { table.unpack(thirds, index, index + take - 1) }
		index, rest = index + take, rest - take
	end
	if tail > 0 then
		packed[#packed + 1] = { corner = true, table.unpack(thirds, index, index + tail - 1) }
	end
	return packed
end

local function layout(tiles)
	local full = WIDTH - PADDING * 2
	local groups = grouped(tiles)
	local stacked = {}
	for index = 1, #groups.halves, 2 do
		stacked[#stacked + 1] = { groups.halves[index], groups.halves[index + 1] }
	end
	for _, tile in ipairs(groups.fulls) do
		stacked[#stacked + 1] = { tile }
	end
	for _, row in ipairs(short_rows(groups.thirds)) do
		stacked[#stacked + 1] = row
	end
	local placed, top = {}, HEADER_HEIGHT
	for _, row in ipairs(stacked) do
		local slots = row.corner and COLUMNS - 1 or #row
		local start = row.corner and (PADDING + third_width() + TILE_GAP) or PADDING
		local room = row.corner and (full - third_width() - TILE_GAP) or full
		local width = (room - TILE_GAP * (slots - 1)) / slots
		if row.corner and #row == 1 then
			width = room
		end
		local height = 0
		for _, tile in ipairs(row) do
			height = math.max(height, arrange(tile, width))
		end
		for index, tile in ipairs(row) do
			arrange(tile, width)
			placed[#placed + 1] =
				{ tile = tile, x = start + (index - 1) * (width + TILE_GAP), y = top, w = width, h = height }
		end
		top = top + height + TILE_GAP
	end
	return placed, top - TILE_GAP + PADDING
end

local function add(items, item)
	items[#items + 1] = item
end

local function text_item(x, y, width, text, size, color, extra)
	local item = { type = "text", x = x, y = y, width = width, text = text, size = size, color = color }
	for key, entry in pairs(extra or {}) do
		item[key] = entry
	end
	return item
end

local function draw_tile(items, placed)
	local tile, x, inner = placed.tile, placed.x + TILE_PADDING, placed.w - TILE_PADDING * 2
	add(
		items,
		{ type = "rect", x = placed.x, y = placed.y, w = placed.w, h = placed.h, radius = 7, color = COLORS.tile }
	)
	local y = placed.y + TILE_PADDING
	add(items, text_item(x, y, inner, tile.label, LABEL_SIZE, COLORS.label))
	y = y + line_height(LABEL_SIZE)
	for index, line in ipairs(tile.value_lines) do
		add(items, text_item(x, y, inner, line, VALUE_SIZE, tile.value_color, { weight = "semibold" }))
		if index == 1 and tile.inline then
			add(
				items,
				text_item(x, y + 2, inner, tile.inline.text, DETAIL_SIZE, tile.inline.color, { align = "right" })
			)
		end
		y = y + line_height(VALUE_SIZE)
	end
	for _, line in ipairs(tile.detail_lines) do
		add(items, text_item(x, y, inner, line.text, DETAIL_SIZE, line.color))
		y = y + line_height(DETAIL_SIZE)
	end
end

local function pill(items, x, y, text, color, size)
	local width = math.ceil(utf8.len(text) * size * 0.62) + 16
	add(items, { type = "rect", x = x, y = y, w = width, h = size + 8, radius = (size + 8) / 2, color = color })
	add(items, text_item(x, y + 3, width, text, size, COLORS.value, { weight = "bold", align = "center" }))
	return width
end

local function plugin_tile(environment)
	local enabled, installed, failed = plugin_counts(environment)
	local spans = { value(string.format("%d/%d enabled", enabled, installed)) }
	if #failed > 0 then
		spans[#spans + 1] = colored(#failed .. " failing: " .. table.concat(failed, ", "), COLORS.bad)
	end
	return { "Plugins", spans }
end

local function healthy(environment)
	local _, _, failed = plugin_counts(environment)
	return #failed == 0 and not present(environment.last_report)
end

local function capitalized(label)
	if label == "astral" or label == "https" then
		return label == "https" and "HTTPS" or label
	end
	return label:sub(1, 1):upper() .. label:sub(2)
end

local function card_tiles(environment)
	local tiles = {}
	for _, row in ipairs(rows(environment)) do
		if row[1] ~= "failed" then
			row[1] = capitalized(row[1])
			tiles[#tiles + 1] = tile_of(row)
		end
	end
	if present(environment.jailbreak) then
		tiles[#tiles + 1] = tile_of({ "Jailbreak", { value(environment.jailbreak) } })
	end
	tiles[#tiles + 1] = tile_of(plugin_tile(environment))
	return tiles
end

local function draw_card(environment, stamp)
	local placed, height = layout(card_tiles(environment))
	local items = {}
	add(items, { type = "rect", x = 0, y = 0, w = WIDTH, h = height, radius = 12, color = COLORS.card })
	add(items, text_item(PADDING, PADDING - 3, 64, "astral", 18, COLORS.value, { weight = "bold" }))
	local x = PADDING + 60
	x = x + pill(items, x, PADDING, environment.astral or "?", COLORS.accent, 10) + 6
	local trailing = joined({ present(environment.commit) and ("@" .. environment.commit) or nil, stamp })
	local good = healthy(environment)
	local status = good and "Healthy" or "Issues found"
	local status_width = math.ceil(utf8.len(status) * 10 * 0.62) + 16
	add(
		items,
		text_item(x, PADDING + 3, WIDTH - PADDING - status_width - 6 - x, trailing, 10, COLORS.dim, { mono = true })
	)
	pill(items, WIDTH - PADDING - status_width, PADDING, status, good and COLORS.good or COLORS.bad, 10)
	for _, entry in ipairs(placed) do
		draw_tile(items, entry)
	end
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
