astral.ui.message_color(function(message)
	if rawget(message, "reply_to_me") == true then
		return astral.Color.Blurple:with_alpha(96)
	end
	return nil
end)
