local REFRESH_MS = 60000
local MAX_EMOJI = 48

local by_guild = {}
local everywhere = { names = nil, loaded = 0 }

local function names_of(guild)
	local names = {}
	for _, emoji in ipairs(guild and guild.emojis or {}) do
		if names[emoji.name] == nil then
			names[emoji.name] = emoji
		end
	end
	return names
end

local function fresh(entry)
	return entry ~= nil and astral.time.now_ms() - entry.loaded < REFRESH_MS
end

local function guild_names(guild_id)
	local entry = by_guild[guild_id]
	if not fresh(entry) then
		entry = { names = names_of(astral.discord.guild(guild_id)), loaded = astral.time.now_ms() }
		by_guild[guild_id] = entry
	end
	return entry.names
end

local function all_names()
	if not fresh(everywhere) or everywhere.names == nil then
		local names = {}
		for _, guild in ipairs(astral.discord.guilds()) do
			for name, emoji in pairs(guild_names(guild.id)) do
				if names[name] == nil then
					names[name] = emoji
				end
			end
		end
		everywhere = { names = names, loaded = astral.time.now_ms() }
	end
	return everywhere.names
end

local function named(name, guild_id)
	local emoji = guild_id and guild_names(guild_id)[name]
	return emoji or all_names()[name]
end

local function url(id, animated)
	return "https://cdn.discordapp.com/emojis/" .. id .. (animated and ".gif" or ".png") .. "?size=96"
end

local function runs(text, context)
	local found = {}
	local guild_id = type(context) == "table" and context.guild_id or nil
	local cursor = 1
	while cursor <= #text and #found < MAX_EMOJI do
		local start, stop, animated, _, id = text:find("<(a?):([%w_]+):(%d+)>", cursor)
		local short_start, short_stop, name = text:find(":([%w_]+):", cursor)
		if short_start and (not start or short_start < start) then
			local emoji = #name >= 2 and named(name, guild_id) or nil
			if emoji then
				found[#found + 1] = {
					start = short_start,
					length = short_stop - short_start + 1,
					style = "image",
					image = url(emoji.id, emoji.animated),
				}
				cursor = short_stop + 1
			else
				cursor = short_stop
			end
		elseif start then
			found[#found + 1] =
				{ start = start, length = stop - start + 1, style = "image", image = url(id, animated == "a") }
			cursor = stop + 1
		else
			break
		end
	end
	return found
end

return { runs = runs }
