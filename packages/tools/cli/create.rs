use std::path::{Path, PathBuf};

use regex::Regex;

use crate::files;
use crate::flags::{Flags, environment};
use crate::repo;

const MAIN: &str = r##"local Pill, Shape = astral.Pill, astral.Shape
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
"##;

const TEST: &str = r#"local function englishStrings()
	local strings = {}
	local folder = arg[1]:match("(.*/)") or "./"
	local file = io.open(folder .. "i18n/en-US.toml")
	if file then
		for line in file:lines() do
			local key, value = line:match('^([%w_]+) = "(.*)"$')
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
print("SLUG ok")
"#;

pub fn manifest(id: &str, name: &str, author: &str) -> String {
    format!(
        "id = {id:?}\nname = {name:?}\ndescription = \"Describe what {name} does in one sentence.\"\nversion = \"0.0.1\"\nicon = \"IconPuzzle\"\nmain = \"main.lua\"\npermissions = []\n\n[[authors]]\nname = {author:?}\n"
    )
}

fn sdk_directory() -> PathBuf {
    let data = environment("XDG_DATA_HOME");
    if data.is_empty() {
        PathBuf::from(environment("HOME")).join(".local/share/astral/sdk")
    } else {
        PathBuf::from(data).join("astral/sdk")
    }
}

pub fn install_sdk() -> Result<PathBuf, String> {
    let target = sdk_directory();
    if target.exists() {
        std::fs::remove_dir_all(&target).map_err(|error| error.to_string())?;
    }
    let source = repo::path(&["packages", "sdk"]);
    let mut result = Ok(());
    files::walk(&source, &|_| false, &mut |file| {
        if result.is_ok()
            && let Ok(relative) = file.strip_prefix(&source)
        {
            result = files::copy(file, &target.join(relative));
        }
    });
    result.map(|_| target)
}

pub fn sdk(arguments: &[String]) -> Result<(), String> {
    if arguments != ["install"] {
        return Err("usage: astral sdk install".into());
    }
    let target = install_sdk()?;
    println!(
        "installed the astral SDK in {}\nadd {:?} to workspace.library in your .luarc.json, or point Lua.workspace.userThirdParty at {:?}",
        target.display(),
        repo::text(&target.join("library")),
        repo::text(target.parent().unwrap_or(Path::new("/")))
    );
    Ok(())
}

pub fn run(arguments: &[String]) -> Result<(), String> {
    let flags = Flags::new("create")
        .string("name", "")
        .string("author", "adrian")
        .string("dir", "")
        .parse(arguments)?;
    let pattern = Regex::new(r"^[a-z0-9]+(\.[a-z0-9-]+)+$").expect("pattern");
    let [id] = flags.rest.as_slice() else {
        return Err("usage: astral create [--name <name>] [--author <name>] [--dir <directory>] <id like astral.my-plugin>".into());
    };
    if !pattern.is_match(id) {
        return Err("usage: astral create [--name <name>] [--author <name>] [--dir <directory>] <id like astral.my-plugin>".into());
    }
    let slug = id.rsplit('.').next().unwrap_or(id);
    let mut name = flags.get("name");
    if name.is_empty() {
        name = slug
            .split('-')
            .map(|word| {
                let mut characters = word.chars();
                characters
                    .next()
                    .map(|first| first.to_uppercase().chain(characters).collect())
                    .unwrap_or_default()
            })
            .collect::<Vec<String>>()
            .join(" ");
    }
    let in_repository = flags.get("dir").is_empty();
    let folder = if in_repository {
        repo::path(&["packages", "addons", "plugins", slug])
    } else {
        PathBuf::from(flags.get("dir"))
    };
    if folder.exists() {
        return Err(format!("{} already exists", folder.display()));
    }
    let mut created = vec![
        (
            "manifest.toml".to_owned(),
            manifest(id, &name, &flags.get("author")),
        ),
        ("main.lua".to_owned(), MAIN.to_owned()),
        (
            "i18n/en-US.toml".to_owned(),
            format!(
                "GENERAL = \"General\"\nSHOW_PILL = \"Show the pill\"\nSHOW_PILL_SUBTITLE = \"Draw a pill after every message author\"\nPILL = \"{name}\"\n"
            ),
        ),
        (format!("tests/{slug}-test.lua"), TEST.replace("SLUG", slug)),
    ];
    if !in_repository {
        let sdk = install_sdk()?;
        created.push((
            ".luarc.json".to_owned(),
            format!(
                "{{\n\t\"$schema\": \"https://raw.githubusercontent.com/LuaLS/vscode-lua/master/setting/schema.json\",\n\t\"runtime.version\": \"Lua 5.4\",\n\t\"workspace.library\": [{:?}],\n\t\"workspace.ignoreDir\": [\"tests\"],\n\t\"diagnostics.globals\": [\"astral\", \"arg\"]\n}}\n",
                repo::text(&sdk.join("library"))
            ),
        ));
    }
    for (path, contents) in created {
        let target = folder.join(path);
        std::fs::create_dir_all(target.parent().unwrap_or(Path::new("/")))
            .map_err(|error| error.to_string())?;
        std::fs::write(target, contents).map_err(|error| error.to_string())?;
    }
    println!("created {}", folder.display());
    if in_repository {
        println!("next: astral test, then astral sign");
    }
    Ok(())
}
