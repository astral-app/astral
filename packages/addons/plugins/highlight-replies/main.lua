astral.ui.style_text(function(message)
	if message.kind == astral.TextKind.Message and rawget(message, "reply_to_me") == true then
		return astral.Color.Blurple:with_alpha(96)
	end
	return nil
end)
