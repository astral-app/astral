local themes = require("lib.themes")
local devicons = require("lib.devicons")
local t = astral.i18n.t

local THEME_REPO =
	"https://cdn.jsdelivr.net/gh/shikijs/textmate-grammars-themes@bc5436518111d87ea58eb56d97b3f9bec30e6b83"
local THEME_URL = THEME_REPO .. "/packages/tm-themes/themes/%s.json"
local LINE_NUMBERS = { light = 0xFF8A8A8A, dark = 0xFF858585 }
local FALLBACK = {
	light = '{"type":"light","colors":{"editor.background":"#ffffff","editor.foreground":"#333333"}}',
	dark = '{"type":"dark","colors":{"editor.background":"#1e1e1e","editor.foreground":"#d4d4d4"}}',
}
local ALIASES = {
	cc = "cpp",
	cxx = "cpp",
	golang = "go",
	h = "c",
	jsonc = "json",
	jsx = "javascript",
	kts = "kotlin",
	md = "markdown",
	shell = "bash",
	sh = "bash",
	yml = "yaml",
}
local LABELS = {
	bash = "Bash",
	c = "C",
	cpp = "C++",
	css = "CSS",
	go = "Go",
	html = "HTML",
	java = "Java",
	javascript = "JavaScript",
	js = "JavaScript",
	json = "JSON",
	kotlin = "Kotlin",
	markdown = "Markdown",
	python = "Python",
	py = "Python",
	rust = "Rust",
	rs = "Rust",
	sql = "SQL",
	swift = "Swift",
	tsx = "TSX",
	ts = "TypeScript",
	typescript = "TypeScript",
	yaml = "YAML",
}

local function label_of(id)
	return (id:gsub("-", " "):gsub("(%a)([%w]*)", function(first, rest)
		return first:upper() .. rest
	end))
end

local theme_options = {}
for _, id in ipairs(themes) do
	theme_options[#theme_options + 1] = astral.settings.option(id, label_of(id))
end

local appearance = astral.settings.section(t("APPEARANCE"))
appearance:choice("theme", t("THEME"), theme_options, "dark-plus", t("THEME_SUBTITLE"))
appearance:choice("useDevIcon", t("LANGUAGE_ICONS"), {
	astral.settings.option("disabled", t("ICONS_DISABLED")),
	astral.settings.option("greyscale", t("ICONS_GREYSCALE")),
	astral.settings.option("colored", t("ICONS_COLORED")),
}, "greyscale", t("LANGUAGE_ICONS_SUBTITLE"))
appearance:text("customTheme", t("CUSTOM_THEME_URL"), "", t("CUSTOM_THEME_URL_SUBTITLE"))

local card = astral.settings.section(t("CARD"))
card:number("bgOpacity", t("BACKGROUND_OPACITY"), { min = 0, max = 100, step = 5, default = 100 })

local loaded = {}
local loading = {}

local function theme_url(source)
	if source:sub(1, 8) == "https://" then
		return source
	end
	return THEME_URL:format(source)
end

local function load_theme(source)
	if loaded[source] then
		return loaded[source]
	end
	local cached = astral.storage.get("theme:" .. source)
	if type(cached) == "string" and cached ~= "" then
		loaded[source] = cached
		return cached
	end
	if not loading[source] then
		loading[source] = true
		astral.net.request({ url = theme_url(source), timeout_ms = 15000 }, function(response)
			loading[source] = nil
			if
				response
				and response.status == 200
				and type(response.body) == "string"
				and response.body:sub(1, 1) == "{"
			then
				loaded[source] = response.body
				astral.storage.set("theme:" .. source, response.body)
				astral.ui.refresh()
			end
		end)
	end
	return nil
end

local function active_theme()
	local value = astral.settings.values.customTheme
	local custom = (type(value) == "string" and value or ""):match("^%s*(.-)%s*$")
	if custom:match("^https://.+%.json") then
		return custom
	end
	return astral.settings.values.theme or "dark-plus"
end

local function fences(content)
	local languages = {}
	for language in content:gmatch("```([^\n`]*)\n") do
		languages[#languages + 1] = language:match("^%s*(%S*)"):lower()
	end
	return languages
end

local RETRY_MS = 500
local RETRIES = 6
local retries = {}

local function retry_later(message_id)
	local count = retries[message_id] or 0
	if count >= RETRIES then
		return
	end
	retries[message_id] = count + 1
	astral.time.after(RETRY_MS, function()
		astral.ui.refresh()
	end)
end

local function language_of(block)
	local message = block.channel_id and astral.discord.raw_message(block.channel_id, block.message_id)
	if type(message) ~= "table" or type(message.content) ~= "string" then
		retry_later(block.message_id)
		return ""
	end
	retries[block.message_id] = nil
	return fences(message.content)[block.index + 1] or ""
end

local function with_alpha(color, opacity)
	local alpha = math.floor(((color >> 24) & 0xFF) * opacity / 100 + 0.5)
	return (color & 0xFFFFFF) | (alpha << 24)
end

local function icon_for(language)
	local name = devicons.languages[ALIASES[language] or language]
	local image = name and devicons.images[name]
	return image and ("data:image/png;base64," .. image) or ""
end

local CODE_SIZE = 12
local CODE_LINE_HEIGHT = 14.5
local HEADER_HEIGHT = 34
local LABEL_SIZE = 11
local ICON_SIZE = 16
local SIDE = 12
local CORNER = 6

local function runs_of(line, foreground)
	local runs = {}
	for _, run in ipairs(line) do
		local text, color, flags = run[1], run[2], run[3] or 0
		if type(text) == "string" and text ~= "" then
			runs[#runs + 1] = {
				text = text,
				color = color or foreground,
				bold = flags & astral.TextStyle.Bold ~= 0,
				italic = flags & astral.TextStyle.Italic ~= 0,
				underline = flags & astral.TextStyle.Underline ~= 0,
			}
		end
	end
	if #runs == 0 then
		runs[1] = { text = " ", color = foreground }
	end
	return runs
end

local function code_text(runs, foreground, grow)
	return {
		type = astral.InsertNodeType.Text,
		runs = runs,
		font = "monospace",
		size = CODE_SIZE,
		line_height = CODE_LINE_HEIGHT,
		color = foreground,
		grow = grow,
	}
end

local function body(lines, background, foreground, numbers)
	local width = math.max(#tostring(#lines), 2)
	local rows = {}
	for index, line in ipairs(lines) do
		local cells = {}
		if numbers then
			cells[1] = code_text({ { text = string.format("%" .. width .. "d ", index), color = numbers } }, foreground)
		end
		cells[#cells + 1] = code_text(runs_of(line, foreground), foreground, true)
		rows[index] = { type = astral.InsertNodeType.View, row = true, align = astral.Align.Start, children = cells }
	end
	return {
		type = astral.InsertNodeType.View,
		background = background,
		padding = { SIDE, 8, SIDE, 10 },
		radius = { 0, 0, CORNER, CORNER },
		fill = true,
		children = rows,
	}
end

local function header(label, icon, tint, background, foreground)
	local children = {}
	if icon ~= "" then
		children[1] = {
			type = astral.InsertNodeType.Image,
			icon = icon,
			width = ICON_SIZE,
			height = ICON_SIZE,
			margin = { 0, 0, 8, 0 },
			color = tint,
		}
	end
	children[#children + 1] = {
		type = astral.InsertNodeType.Text,
		text = label,
		color = foreground,
		size = LABEL_SIZE,
		weight = 600,
		max_lines = 1,
		grow = true,
	}
	children[#children + 1] = {
		type = astral.InsertNodeType.View,
		action = "copy",
		padding = { 8, 8, 0, 8 },
		children = {
			{
				type = astral.InsertNodeType.Text,
				text = t("COPY"),
				color = foreground,
				size = LABEL_SIZE,
				weight = 500,
				max_lines = 1,
			},
		},
	}
	return {
		type = astral.InsertNodeType.View,
		row = true,
		fill = true,
		height = HEADER_HEIGHT,
		padding = { SIDE, 0, SIDE, 0 },
		background = background,
		radius = { CORNER, CORNER, 0, 0 },
		children = children,
	}
end

---@param block astral.CodeBlock
local function render(block)
	local source = active_theme()
	local language = language_of(block)
	local theme = load_theme(source)
	local result = astral.code.highlight(block.code, ALIASES[language] or language, theme or FALLBACK.dark)
	local appearance_kind = result.light and "light" or "dark"
	local opacity = astral.settings.values.bgOpacity or 100
	local background = with_alpha(result.background, opacity)
	local mode = astral.settings.values.useDevIcon or "greyscale"
	local label = LABELS[language]
		or (result.found and result.language)
		or (language ~= "" and language)
		or t("PLAIN_TEXT")
	local icon = mode ~= "disabled" and icon_for(language) or ""
	local tint = mode == "greyscale" and result.foreground or nil
	return {
		type = astral.InsertNodeType.View,
		margin = { 0, 8, 0, 0 },
		fill = true,
		children = {
			header(label, icon, tint, background, result.foreground),
			body(result.lines, background, result.foreground, LINE_NUMBERS[appearance_kind]),
		},
	}
end

---@param block astral.CodeBlock
---@param action string
local function on_press(block, action)
	if action ~= "copy" then
		return
	end
	if astral.ui.copy(block.code) == false then
		astral.ui.toast(t("SHIKI_CODEBLOCKS"), t("COULD_NOT_COPY"))
		return
	end
	astral.ui.toast(t("SHIKI_CODEBLOCKS"), t("CODE_COPIED"))
end

astral.ui.insert("code:", render, on_press)

for _, key in ipairs({ "theme", "customTheme", "bgOpacity", "useDevIcon" }) do
	astral.settings.on_change(key, function()
		astral.ui.refresh()
	end)
end
