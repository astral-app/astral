local RANDOM_CHARACTERS = "0123456789bdfhjkmnpqrstvwxz"
local DEFAULT_NAME = "image"
local t = astral.i18n.t

local section = astral.settings.section(t("METHOD"))
section:switch("anonymise", t("ANONYMISE_BY_DEFAULT"), true)
section:choice("method", t("METHOD"), {
	astral.settings.option("random", t("METHOD_RANDOM")),
	astral.settings.option("consistent", t("METHOD_CONSISTENT")),
	astral.settings.option("timestamp", t("METHOD_TIMESTAMP")),
}, "random")
astral.settings.number("length", t("LENGTH"), { min = 1, max = 32, step = 1, default = 7, section = t("METHOD") })
section:text("name", t("FILE_NAME"), DEFAULT_NAME, { subtitle = t("FILE_NAME_SUBTITLE"), max_length = 64 })

local function extension(filename)
	local tarball = filename:match("(%.tar%.%w+)$")
	return tarball or filename:match("(%.[^.]*)$") or ""
end

local function random_name(length)
	local characters = {}
	for index = 1, length do
		local at = math.random(#RANDOM_CHARACTERS)
		characters[index] = RANDOM_CHARACTERS:sub(at, at)
	end
	return table.concat(characters)
end

local function anonymise(filename)
	local method = astral.settings.get("method", "random")
	local suffix = extension(filename)
	if method == "consistent" then
		local name = tostring(astral.settings.get("name", DEFAULT_NAME) or ""):match("^%s*(.-)%s*$")
		return (name ~= "" and name or DEFAULT_NAME) .. suffix
	elseif method == "timestamp" then
		return tostring(astral.time.now_ms()) .. suffix
	end
	local length = math.floor(tonumber(astral.settings.get("length", 7)) or 7)
	return random_name(math.max(1, math.min(32, length))) .. suffix
end

local renamed = {}

local function rename_all(list)
	local changed = false
	for _, file in ipairs(type(list) == "table" and list or {}) do
		if type(file) == "table" and type(file.filename) == "string" then
			local key = file.uploaded_filename or file.id or file.filename
			renamed[key] = renamed[key] or anonymise(file.filename)
			file.filename = renamed[key]
			changed = true
		end
	end
	return changed
end

local function rewrite(field)
	return function(request)
		if not astral.settings.get("anonymise", true) then
			return nil
		end
		if not tostring(request.headers["content-type"] or ""):find("application/json", 1, true) then
			return nil
		end
		local body = astral.json.decode(request.body)
		if type(body) ~= "table" or not rename_all(body[field]) then
			return nil
		end
		return { body = astral.json.encode(body) }
	end
end

astral.net.intercept({ method = "POST", path = "^/api/v%d+/channels/%d+/attachments$", request = rewrite("files") })
astral.net.intercept({ method = "POST", path = "^/api/v%d+/channels/%d+/messages$", request = rewrite("attachments") })
