local MAXIMUM_TRACKED = 500
local MUTED = 0xFFF0B232

local settings = astral.settings.section("Indicator")
settings:number("threshold", "Latency threshold", {
	min = 1,
	max = 10,
	step = 1,
	default = 2,
	subtitle = "Seconds a message must take to send before it's marked",
})
settings:switch("milliseconds", "Show milliseconds", false, "Shows 2.48s instead of 2s")
settings:switch("ignore_self", "Ignore my messages", false, "Never marks messages you sent")

local latencies, order = {}, {}
local current_user

local function snowflake_time(id)
	local value = math.tointeger(tonumber(id))
	return value and (value >> 22) or nil
end

local function remember(message_id, latency)
	if not latencies[message_id] then
		order[#order + 1] = message_id
		if #order > MAXIMUM_TRACKED then
			latencies[table.remove(order, 1)] = nil
		end
	end
	latencies[message_id] = latency
end

local function measure(data)
	if type(data) ~= "table" or type(data.nonce) ~= "string" or type(data.id) ~= "string" then
		return nil
	end
	local sent, received = snowflake_time(data.nonce), snowflake_time(data.id)
	if not sent or not received then
		return nil
	end
	local latency = received - sent
	if latency < 0 or latency > 24 * 60 * 60 * 1000 then
		return nil
	end
	return latency
end

local function label(latency)
	if astral.settings.get("milliseconds", false) then
		return string.format("%.2fs", latency / 1000)
	end
	return string.format("%ds", latency // 1000)
end

astral.events.on("gateway", function(event)
	if event.type == "READY" and type(event.data.user) == "table" then
		current_user = event.data.user.id
		return nil
	end
	local data = event.data
	local latency = measure(data)
	if not latency or latency < (astral.settings.get("threshold", 2) or 2) * 1000 then
		return nil
	end
	local author = type(data.author) == "table" and data.author.id or nil
	if astral.settings.get("ignore_self", false) and author and author == current_user then
		return nil
	end
	remember(data.id, latency)
	astral.ui.refresh()
	return nil
end, { "READY", "MESSAGE_CREATE" })

astral.ui.insert({ id = "message.timestamp:" }, function(node)
	local latency = latencies[node.key]
	if not latency then
		return nil
	end
	return { type = "text", text = "  ⏳ " .. label(latency), color = MUTED, size = 12 }
end)
