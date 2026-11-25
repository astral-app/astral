local DAY_MS = 24 * 60 * 60 * 1000
local DISCORD_EPOCH = 1420070400000

local settings = astral.settings.section("Sprout")
settings:number(
	"days",
	"Account age",
	{ min = 1, max = 90, step = 1, default = 7, subtitle = "Days a new account keeps the sprout" }
)

local function created_at(user_id)
	local id = math.tointeger(tonumber(user_id))
	return id and (id >> 22) + DISCORD_EPOCH or nil
end

astral.ui.decorate(astral.Target.MessageAuthor, function(node)
	local created = node.user_id and created_at(node.user_id)
	if not created or node:is_bot() then
		return nil
	end
	local age = astral.time.now_ms() - created
	if age < 0 or age > (astral.settings.get("days", 7) or 7) * DAY_MS then
		return nil
	end
	return astral.Pill(astral.i18n.t("NEW_ACCOUNT"), { icon = "sprout.png" })
end)
