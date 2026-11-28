local CROWN = "res://rn_ic_crown_16px"
local GOLD = "#F0B232"

astral.ui.decorate(astral.Target.MessageAuthor, function(node)
	if not node.user_id then
		return nil
	end
	local guild = node:guild()
	if not guild or guild.owner_id ~= node.user_id then
		return nil
	end
	return astral.Pill(astral.i18n.t("SERVER_OWNER"), { icon = CROWN, tint = GOLD }):on_press("crown")
end, function()
	astral.ui.tooltip(astral.i18n.t("SERVER_OWNER"))
end)
