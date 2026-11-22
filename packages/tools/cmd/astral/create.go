package main

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"strings"
)

var pluginIDPattern = regexp.MustCompile(`^[a-z0-9]+(\.[a-z0-9-]+)+$`)

const mainTemplate = `local Pill, Shape = astral.Pill, astral.Shape
local settings = astral.settings.values
local t = astral.i18n.t

local general = astral.settings.section(t("GENERAL"))
general:switch("showPill", t("SHOW_PILL"), true, t("SHOW_PILL_SUBTITLE"))

astral.ui.decorate(astral.Target.MessageAuthor, function(node)
	if not settings.showPill or not node.user_id then
		return nil
	end
	return Pill(t("PILL")):color("#5865F2"):shaped(Shape.Pill)
end)
`

const translationsTemplate = `GENERAL = "General"
SHOW_PILL = "Show the pill"
SHOW_PILL_SUBTITLE = "Draw a pill after every message author"
PILL = "%s"
`

const testTemplate = `local function englishStrings()
	local strings = {}
	local folder = arg[1]:match("(.*/)") or "./"
	local file = io.open(folder .. "i18n/en-US.toml")
	if file then
		for line in file:lines() do
			local key, value = line:match('^([%%w_]+) = "(.*)"$')
			if key then
				strings[key] = value
			end
		end
		file:close()
	end
	return strings
end

local english = englishStrings()
local decorator
astral = {
	log = function() end,
	i18n = {
		t = function(key)
			return english[key] or key
		end,
	},
	settings = {
		switch = function(_, _, default)
			return default
		end,
		choice = function(_, _, _, default)
			return default
		end,
	},
	ui = {
		decorate = function(_, fn)
			decorator = fn
		end,
		relabel = function() end,
	},
	discord = { permission = {} },
}
dofile(arg[2])
dofile(arg[1])

local pills = decorator({ user_id = "1" })
assert(#pills == 1 and pills[1].text == english.PILL, "one pill for an author")
astral.on_setting("showPill", false)
assert(decorator({ user_id = "1" }) == nil, "no pill when switched off")
print("%s ok")
`

const luarcTemplate = `{
	"$schema": "https://raw.githubusercontent.com/LuaLS/vscode-lua/master/setting/schema.json",
	"runtime.version": "Lua 5.4",
	"workspace.library": [%q],
	"workspace.ignoreDir": ["tests"],
	"diagnostics.globals": ["astral", "arg"]
}
`

func manifestTemplate(id string, name string, author string) string {
	return fmt.Sprintf(`id = %q
name = %q
description = "Describe what %s does in one sentence."
version = "0.0.1"
icon = "IconPuzzle"
main = "main.lua"
permissions = []

[[authors]]
name = %q
`, id, name, name, author)
}

func writeFiles(folder string, files map[string]string) error {
	for path, content := range files {
		target := filepath.Join(folder, path)
		if err := os.MkdirAll(filepath.Dir(target), 0o755); err != nil {
			return err
		}
		if err := os.WriteFile(target, []byte(content), 0o644); err != nil {
			return err
		}
	}
	return nil
}

func runCreate(args []string) error {
	flags := newFlags("create")
	name := flags.String("name", "", "display name (default: from the id)")
	author := flags.String("author", "adrian", "author name")
	directory := flags.String("dir", "", "where to create it (default: packages/plugins/<slug> in this repository)")
	if err := flags.Parse(args); err != nil {
		return err
	}
	if flags.NArg() != 1 || !pluginIDPattern.MatchString(flags.Arg(0)) {
		return errors.New("usage: astral create [--name <name>] [--author <name>] [--dir <directory>] <id like astral.my-plugin>")
	}
	id := flags.Arg(0)
	slug := id[strings.LastIndex(id, ".")+1:]
	if *name == "" {
		words := strings.Split(slug, "-")
		for index, word := range words {
			words[index] = strings.ToUpper(word[:1]) + word[1:]
		}
		*name = strings.Join(words, " ")
	}
	folder := *directory
	inRepository := folder == ""
	if inRepository {
		folder = repoPath("packages", "plugins", slug)
	}
	if fileExists(folder) {
		return errors.New(folder + " already exists")
	}
	files := map[string]string{
		"manifest.toml":               manifestTemplate(id, *name, *author),
		"main.lua":                    mainTemplate,
		"i18n/en-US.toml":             fmt.Sprintf(translationsTemplate, *name),
		"tests/" + slug + "-test.lua": fmt.Sprintf(testTemplate, slug),
	}
	if !inRepository {
		sdk, err := installSDK()
		if err != nil {
			return err
		}
		files[".luarc.json"] = fmt.Sprintf(luarcTemplate, filepath.Join(sdk, "library"))
	}
	if err := writeFiles(folder, files); err != nil {
		return err
	}
	fmt.Println("created " + folder)
	if inRepository {
		fmt.Println("next: astral test, then astral sign")
	}
	return nil
}
