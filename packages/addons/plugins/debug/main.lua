local WIDTH = 432
local SCALE = 3
local PADDING = 12
local HEADER_HEIGHT = 34
local FONT_SIZE = 11.5
local CHAR_WIDTH = FONT_SIZE * 0.6
local LINE_HEIGHT = 17
local LABEL_WIDTH = 66
local LABEL_SIZE = 11
local ALT_CLEARANCE = 64
local ALT_HEIGHT = 46
local MAX_ALT_TEXT = 1024
local NOISE_BINARY_PART = { "^%d+ dylibs$", "^%d+ injected$", "^UUID %S+$" }
local NOISE_MARKER = { "^%d+ Hz$" }
local ALT_DROP_ORDER =
	{ "plugins_failed", "markers", "https", "binary", "signer", "install", "source", "framework", "jailbreak" }
local CHANNELS = { canary = "alpha", alpha = "alpha", ptb = "stable", stable = "stable", beta = "beta" }

local COLORS = {
	card = 0xFF1E1F22,
	stripe = 0xFF232428,
	label = 0xFF949BA4,
	value = 0xFFF2F3F5,
	dim = 0xFF949BA4,
	accent = 0xFF5865F2,
	commit = 0xFFA78BFA,
	good = 0xFF23A55A,
	warn = 0xFFF0B232,
	bad = 0xFFDA373C,
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

local function framework_version(framework)
	return present(framework) and (framework:gsub("(%d[%w.]*)%-%d+$", "%1")) or nil
end

local function kept_markers(markers)
	local kept = {}
	for _, marker in ipairs(type(markers) == "table" and markers or {}) do
		local noise = false
		for _, pattern in ipairs(NOISE_MARKER) do
			noise = noise or marker:match(pattern) ~= nil
		end
		if not noise then
			kept[#kept + 1] = marker
		end
	end
	return table.concat(kept, " · ")
end

local function system_spans(environment)
	local system = environment.system or "?"
	if present(environment.os_build) and not system:find("(", 1, true) then
		system = system .. " (" .. environment.os_build .. ")"
	end
	return {
		value(system),
		present(environment.os_beta) and colored(" · beta", COLORS.warn) or nil,
	}
end

local function crash_spans(environment)
	local recent = tonumber(environment.crashes_24h) or 0
	if recent > 0 then
		return {
			colored(recent .. " in 24h", COLORS.bad),
			present(environment.last_report) and dim(" · " .. environment.last_report) or nil,
		}
	end
	return present(environment.last_report) and { colored(environment.last_report, COLORS.warn) } or nil
end

local function rows(environment)
	local list = {}
	local version, build, channel = discord_parts(environment)
	append(list, "injection", {
		value(environment.image or "astral"),
		present(environment.framework) and dim(" · " .. framework_version(environment.framework)) or nil,
	})
	local discord = { value(version), dim(" " .. joined({ channel, build })) }
	for _, span in ipairs(support_spans(environment)) do
		discord[#discord + 1] = span
	end
	append(list, "discord", discord)
	append(list, "source", present(environment.source) and { value(environment.source) } or nil)
	append(list, "device", { value(environment.device_name or environment.device or "?") })
	append(list, "system", system_spans(environment))
	append(list, "install", present(environment.install) and { value((environment.install:gsub("`", ""))) } or nil)
	append(list, "signer", present(environment.signer) and { value(environment.signer) } or nil)
	append(list, "binary", present(environment.binary) and { value(binary_text(environment.binary)) } or nil)
	local markers = kept_markers(environment.markers)
	append(list, "markers", present(markers) and { value(markers) } or nil)
	append(list, "https", https_spans(environment.https))
	append(list, "phoenix", present(environment.phoenix) and {
		value(environment.phoenix:sub(1, 8)),
		present(environment.engine) and environment.engine ~= "Phoenix" and dim(" · " .. environment.engine) or nil,
	} or nil)
	local _, _, failed = plugin_counts(environment)
	append(list, "failing", #failed > 0 and { colored(table.concat(failed, " · "), COLORS.bad) } or nil)
	append(list, "crashes", crash_spans(environment))
	return list
end

local function kept_spans(spans)
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

local function add(items, item)
	items[#items + 1] = item
end

local function runs(items, spans, x, y)
	for _, span in ipairs(spans) do
		add(items, {
			type = astral.ImageItemType.Text,
			x = x,
			y = y,
			text = span.text,
			size = FONT_SIZE,
			mono = true,
			color = span.color,
		})
		x = x + utf8.len(span.text) * CHAR_WIDTH
	end
end

local function span_length(spans)
	local length = 0
	for _, span in ipairs(spans) do
		length = length + utf8.len(span.text)
	end
	return length
end

local function pill(items, x, y, text, color)
	local width = math.ceil(utf8.len(text) * 10 * 0.62) + 14
	add(items, { type = astral.ImageItemType.Rect, x = x, y = y, w = width, h = 17, radius = 8.5, color = color })
	add(items, {
		type = astral.ImageItemType.Text,
		x = x,
		y = y + 2.5,
		width = width,
		text = text,
		size = 10,
		weight = astral.FontWeight.Bold,
		align = astral.TextAlign.Center,
		color = COLORS.value,
	})
	return width
end

local function summary_spans(environment)
	local enabled, installed, failed = plugin_counts(environment)
	local spans = {}
	if present(environment.jailbreak) then
		spans[#spans + 1] = dim(environment.jailbreak:lower() .. "  ")
	end
	local crashes = tonumber(environment.crashes_24h) or 0
	if #failed > 0 then
		spans[#spans + 1] = colored("● " .. #failed .. " failing  ", COLORS.bad)
	elseif crashes > 0 then
		spans[#spans + 1] = colored("● crashing  ", COLORS.bad)
	else
		spans[#spans + 1] = colored("● healthy  ", COLORS.good)
	end
	spans[#spans + 1] = dim(string.format("%d/%d plugins", enabled, installed))
	return spans
end

local function draw_header(items, environment, stamp)
	add(items, {
		type = astral.ImageItemType.Text,
		x = PADDING,
		y = PADDING - 3,
		width = 60,
		text = "astral",
		size = 17,
		weight = astral.FontWeight.Bold,
		color = COLORS.value,
	})
	local x = PADDING + 56 + pill(items, PADDING + 56, PADDING, environment.astral or "?", COLORS.accent) + 6
	local trailing = { present(environment.commit) and colored("@" .. environment.commit, COLORS.commit) or nil }
	trailing[#trailing + 1] = dim((trailing[1] and "  " or "") .. stamp)
	local room = math.floor((WIDTH - PADDING - x) / (10 * 0.6))
	if span_length(trailing) > room then
		trailing = { trailing[1] }
	end
	for _, span in ipairs(trailing) do
		add(items, {
			type = astral.ImageItemType.Text,
			x = x,
			y = PADDING + 3,
			text = span.text,
			size = 10,
			mono = true,
			color = span.color,
		})
		x = x + utf8.len(span.text) * 10 * 0.6
	end
end

local function draw_card(environment, stamp)
	local room = math.floor((WIDTH - PADDING * 2 - LABEL_WIDTH) / CHAR_WIDTH)
	local lines = {}
	for index, row in ipairs(rows(environment)) do
		for line_index, line in ipairs(wrapped(kept_spans(row[2]), room)) do
			lines[#lines + 1] = { label = line_index == 1 and row[1] or "", line = line, stripe = index % 2 == 0 }
		end
	end
	local summary = summary_spans(environment)
	local summary_room = math.floor((WIDTH - PADDING * 2 - ALT_CLEARANCE) / CHAR_WIDTH)
	local height = HEADER_HEIGHT + #lines * LINE_HEIGHT + ALT_HEIGHT
	local items =
		{ { type = astral.ImageItemType.Rect, x = 0, y = 0, w = WIDTH, h = height, radius = 12, color = COLORS.card } }
	draw_header(items, environment, stamp)
	for index, entry in ipairs(lines) do
		local top = HEADER_HEIGHT + (index - 1) * LINE_HEIGHT
		if entry.stripe then
			add(items, {
				type = astral.ImageItemType.Rect,
				x = PADDING - 6,
				y = top,
				w = WIDTH - (PADDING - 6) * 2,
				h = LINE_HEIGHT,
				radius = 4,
				color = COLORS.stripe,
			})
		end
		add(items, {
			type = astral.ImageItemType.Text,
			x = PADDING,
			y = top + 2.5,
			width = LABEL_WIDTH - 6,
			text = entry.label,
			size = LABEL_SIZE,
			color = COLORS.label,
		})
		runs(items, entry.line, PADDING + LABEL_WIDTH, top + 2)
	end
	if span_length(summary) <= summary_room then
		local top = HEADER_HEIGHT + #lines * LINE_HEIGHT
		runs(
			items,
			summary,
			WIDTH - PADDING - span_length(summary) * CHAR_WIDTH,
			top + (ALT_HEIGHT - FONT_SIZE) / 2 - 2
		)
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
	options = { { name = "message", description = "Add a note above the card", type = astral.OptionType.String } },
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
