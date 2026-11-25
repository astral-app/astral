local t = astral.i18n.t

local UNITS = { "KB", "MB", "GB", "TB", "PB", "EB", "ZB", "YB" }
local SCRIM = 0x801E1F22
local INSET = 3

local function format_number(value)
	local text = string.format("%.2f", value):gsub("%.?0+$", "")
	return text
end

local function format_bytes(bytes)
	if bytes <= 0 then
		return "0 " .. t("BYTES")
	end
	local index = math.min(math.floor(math.log(bytes, 1024)), #UNITS)
	if index == 0 then
		return format_number(bytes) .. " " .. t("BYTES")
	end
	return format_number(bytes / 1024 ^ index) .. " " .. UNITS[index]
end

astral.ui.insert({ id = "media-tile-", type = "stack" }, function(node)
	if not node.key:match("^%d+$") then
		return nil
	end
	local media = astral.media.picked("picked://p" .. node.key)
	if not media or not media.size then
		return nil
	end
	return {
		type = "view",
		background = SCRIM,
		radius = 4,
		padding = { 4, 2, 4, 2 },
		margin = { INSET, INSET, 0, 0 },
		position = "end",
		children = {
			{ type = "text", text = format_bytes(media.size), size = 10, weight = 600, color = 0xFFFFFFFF },
		},
	}
end)
