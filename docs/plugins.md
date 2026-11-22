# Plugins

Plugins are Lua 5.4 files. astral runs no native plugin code, so a plugin works the same on jailbroken and sideloaded installs and cannot crash Discord.

## Icons

A manifest's `iconSvg` carries the plugin's icon, as Unbound's catalog does; the loader rasterises it on device into a template image Phoenix tints like its own icons. Without one, `icon` may name a Discord icon (`TextIcon`), drawn from Phoenix's `ic_mana_*` assets, and otherwise the card shows the plug. astral's own icons (`assets/*.svg`) are drawn the same way.

## Authors

Cards show each author's avatar the way Unbound does: Discord's local user cache first, then a `GET /users/{id}` with the signed-in session, cached in `Documents/astral/authors.json` for a week, then Discord's default avatar by the author's position.

## Package

```
plugins/<name>/manifest.json   {"id", "name", "description", "authors": [{"name", "id"}], "version", "runtime": "lua", "main": "main.lua"}
plugins/<name>/main.lua        at most 256 KB
plugins/<name>/tests/*.lua     run by tools/test.sh against a mocked `astral` table
```

Bundled plugins ship in the package's resource directory (`/var/jb/usr/lib/astral` when jailbroken, `Astral.bundle` inside the app when sideloaded). Plugins installed from the marketplace live in `Documents/astral/plugins` and take precedence over bundled ones with the same id.

## Sandbox

Each plugin gets its own `lua_State` with `base`, `table`, `string`, `math`, `utf8` and `coroutine`. `dofile`, `loadfile`, `load`, `require` and `collectgarbage` are removed. A state has an 8 MB heap and a 200,000-instruction budget per call; five errors stop the plugin, and its card shows the error.

## API 1

| Call | Purpose |
| --- | --- |
| `astral.log(...)`, `astral.id`, `astral.api` | Logging and identity |
| `astral.settings.switch(key, title, default, { subtitle, section })` | A switch on the plugin's settings page, grouped by section |
| `astral.settings.choice(key, title, options, default, { section })` | Radio rows; `options` are `{ value, title, subtitle }` |
| `astral.settings.get/set`, `astral.on_setting(key, value)` | Persisted values |
| `astral.ui.decorate("message.author", fn)` | `fn(node)` returns pills `{ text, color, text_color, colors, shape }` shown after the guild tag |
| `astral.ui.relabel("message.author", fn)` | `fn(node)` returns new text for a message author (`node.kind == "author"`) or a reply preview's author (`"reply"`); plugins chain |
| `astral.discord.guild/member/message/user/nickname/channel_guild/permissions` | Read-only views of Discord's own cache; messages carry `type`, `author` and `reference`, permissions apply channel overwrites |
| `astral.discord.permission.*` | Permission bits |

A pill with two or three `colors` draws Phoenix's native `backgroundGradient`; `shape = "op"` draws Discord's original poster style.

## Decorations

The ViewWire rewriter tracks `message.author`, `message.avatar`, `message.timestamp` and `navigation.channel` nodes. The author's user id comes from the avatar image's URL (two levels below the avatar node), the channel from the selected channel row or, before the channel list exists, from the message's row in the cache. Pills are created under ids from `0x7b000000`, inserted before the timestamp, replaced when the row renders again and removed with it.

## Discord cache

`Application Support/store/<account>/cache.sqlite` holds JSON in key-value tables whose keys are snowflakes zero-padded to 20 digits: `guilds` (guild), `members` (guild:user), `channels` (guild:channel), `channel_overwrites` (guild:channel), `messages` (channel:message) and `users` (user).

## Settings UI

The astral category in Discord's settings has astral, Plugins, Design, Marketplace and Updates rows, cloned from Discord's own settings rows. Pages are pushed onto the settings navigator, up to four deep. The Plugins page draws Unbound-style cards: plug tile, name, author avatars, settings and overflow buttons, switch, description and version tag, with filter chips and an install button.

## Themes

A theme maps ARGB colours to ARGB colours. The rewriter replaces matching colour properties in each batch before Discord's host decodes it, so Discord draws its own views in the theme's colours.
