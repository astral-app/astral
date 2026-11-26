local CDN = "https://cdn.discordapp.com"
local DEFAULT_AVATARS = 6
local FACES = 5
local MAX_ENTRIES = 512
local RETRY_AFTER_MS = 3000
local REFRESH_COOLDOWN_MS = 10000
local MAX_RATE_LIMIT_WAIT_MS = 10000
local TOO_MANY_REQUESTS = 429

local entries, entry_count = {}, 0
local queue, queued = {}, {}
local busy = false

local function avatar_url(user)
	if type(user.avatar) == "string" and user.avatar ~= "" then
		return string.format("%s/avatars/%s/%s.webp?size=48", CDN, user.id, user.avatar)
	end
	local number = math.tointeger(tonumber(user.id)) or 0
	return string.format("%s/embed/avatars/%d.png", CDN, (number >> 22) % DEFAULT_AVATARS)
end

local function encoded(text)
	return (text:gsub("[^%w]", function(character)
		return string.format("%%%02X", character:byte())
	end))
end

local function public_users(body)
	local ok, parsed = pcall(astral.json.decode, body or "")
	if not ok or type(parsed) ~= "table" then
		return nil
	end
	local users = {}
	for _, user in ipairs(parsed) do
		if type(user) == "table" and type(user.id) == "string" then
			users[#users + 1] =
				{ id = user.id, avatar = user.avatar, username = user.username, global_name = user.global_name }
			if #users == FACES then
				break
			end
		end
	end
	return #users > 0 and users or nil
end

local function remember(key, entry)
	if not entries[key] then
		entry_count = entry_count + 1
		if entry_count > MAX_ENTRIES then
			entries, entry_count = {}, 1
		end
	end
	entries[key] = entry
end

local function rate_limit_wait(body)
	local ok, parsed = pcall(astral.json.decode, body or "")
	local seconds = ok and type(parsed) == "table" and tonumber(parsed.retry_after) or 0
	return math.min(math.max(math.floor(seconds * 1000), 0), MAX_RATE_LIMIT_WAIT_MS)
end

local pump

local function finish(job, response)
	queued[job.key] = nil
	local users = response.ok and public_users(response.body) or nil
	local previous = entries[job.key]
	remember(job.key, {
		count = job.count,
		users = users or (previous and previous.users),
		fetched = astral.time.now_ms(),
		failed = users == nil and astral.time.now_ms() or nil,
	})
	if users then
		astral.ui.refresh()
	end
	local wait = response.status == TOO_MANY_REQUESTS and rate_limit_wait(response.body) or 0
	if wait > 0 then
		astral.time.after(wait, function()
			busy = false
			pump()
		end)
	else
		busy = false
		pump()
	end
end

pump = function()
	if busy or #queue == 0 then
		return
	end
	local job = table.remove(queue, 1)
	busy = true
	local ok = pcall(function()
		astral.discord.request({ path = job.path }, function(response)
			finish(job, response)
		end)
	end)
	if not ok then
		finish(job, { ok = false, status = 0 })
	end
end

local function lookup(node)
	if not node.channel_id or not node.message_id or not node.emoji or (node.count or 0) == 0 then
		return nil
	end
	local burst = node.burst and 1 or 0
	local key = table.concat({ node.channel_id, node.message_id, node.emoji, burst }, ":")
	local entry = entries[key]
	local now = astral.time.now_ms()
	local stale = not entry or entry.count ~= node.count
	local retry = entry and entry.failed and now - entry.failed >= RETRY_AFTER_MS
	local cooled = not entry or not entry.fetched or now - entry.fetched >= REFRESH_COOLDOWN_MS
	if (stale and cooled or retry) and not queued[key] then
		queued[key] = true
		queue[#queue + 1] = {
			key = key,
			count = node.count,
			path = string.format(
				"/channels/%s/messages/%s/reactions/%s?limit=%d&type=%d",
				node.channel_id,
				node.message_id,
				encoded(node.emoji),
				FACES,
				burst
			),
		}
		pump()
	end
	return entry and entry.users
end

astral.ui.reaction(function(node)
	local users = lookup(node)
	if not users or #users == 0 then
		return nil
	end
	local images = {}
	for _, user in ipairs(users) do
		images[#images + 1] = avatar_url(user)
	end
	return { images = images, total = node.count }
end)
