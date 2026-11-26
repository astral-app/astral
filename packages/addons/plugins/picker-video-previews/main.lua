local t = astral.i18n.t

local PICKED = "picked://"

local playback = astral.settings.section(t("PLAYBACK"))
playback:switch("sound", t("SOUND"), false, t("SOUND_SUBTITLE"))

astral.ui.insert("image:" .. PICKED, function(node)
	if node.parent_id and node.parent_id ~= "" then
		return nil
	end
	local uri = PICKED .. node.key
	local media = astral.media.picked(uri)
	if not media or media.type ~= astral.PickedMediaType.Video then
		return nil
	end
	local url = media.url
	if not url then
		return nil
	end
	return {
		type = astral.InsertNodeType.Player,
		uri = url,
		fill = true,
		fill_height = true,
		radius = 8,
		muted = not astral.settings.values.sound,
	}
end)

astral.settings.on_change("sound", function()
	astral.ui.refresh()
end)
