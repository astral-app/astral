std = "lua54"
max_line_length = 120
globals = { "astral" }
files["plugins/*/tests/*.lua"] = { read_globals = { "arg" } }
