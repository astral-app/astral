local rules = {
	"action_object_map",
	"action_type_map",
	"action_ref_map",
	"spm@*.aliexpress.com",
	"scm@*.aliexpress.com",
	"aff_platform",
	"aff_trace_key",
	"algo_expid@*.aliexpress.*",
	"algo_pvid@*.aliexpress.*",
	"btsid",
	"ws_ab_test",
	"pd_rd_*@amazon.*",
	"_encoding@amazon.*",
	"psc@amazon.*",
	"tag@amazon.*",
	"ref_@amazon.*",
	"pf_rd_*@amazon.*",
	"pf@amazon.*",
	"crid@amazon.*",
	"keywords@amazon.*",
	"sprefix@amazon.*",
	"sr@amazon.*",
	"ie@amazon.*",
	"node@amazon.*",
	"qid@amazon.*",
	"callback@bilibili.com",
	"cvid@bing.com",
	"form@bing.com",
	"sk@bing.com",
	"sp@bing.com",
	"sc@bing.com",
	"qs@bing.com",
	"pq@bing.com",
	"sc_cid",
	"mkt_tok",
	"trk",
	"trkCampaign",
	"ga_*",
	"gclid",
	"gclsrc",
	"hmb_campaign",
	"hmb_medium",
	"hmb_source",
	"spReportId",
	"spJobID",
	"spUserID",
	"spMailingID",
	"itm_*",
	"s_cid",
	"elqTrackId",
	"elqTrack",
	"assetType",
	"assetId",
	"recipientId",
	"campaignId",
	"siteId",
	"mc_cid",
	"mc_eid",
	"pk_*",
	"sc_campaign",
	"sc_channel",
	"sc_content",
	"sc_medium",
	"sc_outcome",
	"sc_geo",
	"sc_country",
	"nr_email_referer",
	"vero_conv",
	"vero_id",
	"yclid",
	"_openstat",
	"mbid",
	"cmpid",
	"cid",
	"c_id",
	"campaign_id",
	"Campaign",
	"hash@ebay.*",
	"fb_action_ids",
	"fb_action_types",
	"fb_ref",
	"fb_source",
	"fbclid",
	"refsrc@facebook.com",
	"hrc@facebook.com",
	"gs_l",
	"gs_lcp@google.*",
	"gws_rd@google.*",
	"ved@google.*",
	"ei@google.*",
	"sei@google.*",
	"gs_gbg@google.*",
	"gs_mss@google.*",
	"gs_rn@google.*",
	"_hsenc",
	"_hsmi",
	"__hssc",
	"__hstc",
	"hsCtaTracking",
	"source@sourceforge.net",
	"position@sourceforge.net",
	"t@*.twitter.com",
	"s@*.twitter.com",
	"ref_*@*.twitter.com",
	"tt_medium",
	"tt_content",
	"lr@yandex.*",
	"redircnt@yandex.*",
	"feature@youtube.com",
	"kw@youtube.com",
	"si@youtu.be",
	"wt_zmc",
	"utm_source",
	"utm_content",
	"utm_medium",
	"utm_campaign",
	"utm_term",
	"si@open.spotify.com",
	"igsh",
	"igshid",
	"share_id@reddit.com",
}

local function wildcard(pattern, value)
	local patternIndex = 1
	local valueIndex = 1
	while true do
		local star = pattern:find("*", patternIndex, true)
		if not star then
			local suffix = pattern:sub(patternIndex)
			if value:sub(valueIndex, valueIndex + #suffix - 1) ~= suffix then
				return false
			end
			return valueIndex + #suffix - 1 == #value
		end
		local literal = pattern:sub(patternIndex, star - 1)
		if value:sub(valueIndex, valueIndex + #literal - 1) ~= literal then
			return false
		end
		valueIndex = valueIndex + #literal
		local nextStar = pattern:find("*", star + 1, true)
		local nextLiteral = pattern:sub(star + 1, nextStar and nextStar - 1 or #pattern)
		if nextLiteral == "" then
			return valueIndex <= #value
		end
		local found = value:find(nextLiteral, valueIndex, true)
		if not found then
			return false
		end
		valueIndex = found + #nextLiteral
		patternIndex = star + 1 + #nextLiteral
		if not nextStar then
			return value:sub(-#nextLiteral) == nextLiteral
		end
	end
end

local function decodeKey(value)
	return value:gsub("%+", " "):gsub("%%(%x%x)", function(hex)
		return string.char(tonumber(hex, 16))
	end)
end

local function ruleMatches(rule, key, host)
	local separator = rule:find("@", 1, true)
	local parameter = separator and rule:sub(1, separator - 1) or rule
	if not wildcard(parameter, key) then
		return false
	end
	if not separator then
		return true
	end
	local pattern = rule:sub(separator + 1):lower()
	local normalizedHost = host:lower():gsub("^www%.", "")
	if pattern:sub(1, 2) == "*." then
		local suffix = pattern:sub(3)
		local candidate = normalizedHost
		while true do
			if wildcard(suffix, candidate) then
				return true
			end
			local dot = candidate:find(".", 1, true)
			if not dot then
				return false
			end
			candidate = candidate:sub(dot + 1)
		end
	end
	return wildcard(pattern, normalizedHost)
end

local function cleanUrl(value)
	local scheme, authority, rest = value:match("^(https?://)([^/%?#]+)(.*)$")
	if not authority then
		return value
	end
	local host = authority:match("([^@]+)$") or authority
	if host:sub(1, 1) == "[" then
		host = host:match("^(%b[])") or host
	else
		host = host:match("^[^:]+") or host
	end
	local beforeFragment, fragment = rest:match("^(.-)(#.*)$")
	if not beforeFragment then
		beforeFragment, fragment = rest, ""
	end
	local path, query = beforeFragment:match("^(.-)%?(.*)$")
	if not query then
		return value
	end
	local kept = {}
	local position = 1
	while position <= #query do
		local querySeparator = query:find("&", position, true)
		local item = query:sub(position, querySeparator and querySeparator - 1 or #query)
		local key = decodeKey(item:match("^([^=]*)") or "")
		local remove = false
		for _, rule in ipairs(rules) do
			if ruleMatches(rule, key, host) then
				remove = true
				break
			end
		end
		if not remove then
			kept[#kept + 1] = item
		end
		if not querySeparator then
			break
		end
		position = querySeparator + 1
	end
	local queryText = table.concat(kept, "&")
	return scheme .. authority .. (path or "") .. (queryText ~= "" and "?" .. queryText or "") .. fragment
end

local function cleanUrls(content)
	local output = {}
	local position = 1
	while true do
		local start = content:find("https?://", position)
		if not start then
			output[#output + 1] = content:sub(position)
			break
		end
		output[#output + 1] = content:sub(position, start - 1)
		local stop = content:find("[%s<>]", start)
		local finish = stop and stop - 1 or #content
		local link = content:sub(start, finish)
		local trailing = link:match("([%.,:;\"')%]%}]+)$") or ""
		local core = #trailing > 0 and link:sub(1, #link - #trailing) or link
		output[#output + 1] = cleanUrl(core) .. trailing
		position = finish + 1
	end
	return table.concat(output)
end

astral.messages.transform(function(content)
	local cleaned = cleanUrls(content)
	if cleaned ~= content then
		return cleaned
	end
end)
