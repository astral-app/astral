# Plugins

Plugins are Lua 5.4 files. astral runs no native plugin code, so a plugin works the same on jailbroken and sideloaded installs and cannot crash Discord.

## Translations

A plugin's strings live in `i18n/<locale>.toml` (flat `KEY = "text"` tables); astral finds them by scanning the folder. `astral.i18n.t(key, { name = value })` returns the string for the best of the device's preferred languages, falling back per key to English and then to the key; `astral.i18n.locale` names the table in use. Translations are part of the plugin's signature, and marketplace installs download and verify them. The two first-party plugins carry Unbound's 32 locales.

## Icons

A manifest's `iconSvg` carries the plugin's icon, as Unbound's catalog does; astral rasterises it on device into a template image Phoenix tints like its own icons. Without one, `icon` may name a Discord icon (`TextIcon`), drawn from Phoenix's `ic_mana_*` assets, and otherwise the card shows the plug. astral's own icons (`assets/*.svg`) are drawn the same way.

## Authors

Cards show each author's avatar the way Unbound does: Discord's local user cache first, then a `GET /users/{id}` with the signed-in session, cached in `Documents/astral/authors.json` for a week, then Discord's default avatar by the author's position.

## Package

```
packages/plugins/<name>/manifest.toml   {"id", "name", "description", "authors": [{"name", "id"}], "version", "main": "main.lua"}
packages/plugins/<name>/main.lua        at most 256 KB
packages/plugins/<name>/**/*.lua        modules, loaded with require("lib.util"), at most 1 MB together
packages/plugins/<name>/tests/*.lua     run by astral test against a mocked `astral` table and the prelude
```

Bundled plugins ship in the package's resource directory (`/var/jb/usr/lib/astral` when jailbroken, `astral.bundle` inside the app when sideloaded). Plugins installed from the marketplace live in `Documents/astral/plugins` and take precedence over bundled ones with the same id.

## Signing

Marketplace plugins and the catalog are signed with astral's Ed25519 key (`keys/plugin-signing.pub`, compiled into astral and checked with Monocypher). A plugin's `signature` is a small TOML file listing the SHA-256 of `manifest.toml`, every Lua file outside `tests/` and every `i18n/*.toml`, with an Ed25519 signature over that list (`astral-plugin-v2`); marketplace installs read it first and download exactly the files it names. The catalog's `index.toml.sig` covers the catalog. `astral sign` signs everything and `astral test` fails on a stale signature. Bundled plugins ship inside the package; anything else that is unsigned stays off, marked "unsigned", unless Developer mode is on. Downloads are verified before they are written.

## Sandbox

Each plugin gets its own `lua_State` with `base`, `table`, `string`, `math`, `utf8` and `coroutine`. `dofile`, `loadfile`, `load`, `require` and `collectgarbage` are removed. A state has an 8 MB heap and a 200,000-instruction budget per call; five errors stop the plugin, and its card shows the error.

## API 1

| Call | Purpose |
| --- | --- |
| `astral.log(...)`, `astral.id`, `astral.api` | Logging and identity |
| `astral.settings.section(title)` | A settings section; `section:switch(key, title, default, subtitle)` and `section:choice(key, title, options, default, subtitle)` declare rows and return the current value |
| `astral.settings.switch/choice(key, title, ..., { subtitle, section })` | The same rows without a section object |
| `astral.settings.option(value, title, subtitle)` | One choice option |
| `astral.settings.number(key, title, { min, max, step, default, subtitle, section })` | A number drawn as Phoenix's slider row; returns the current value |
| `astral.settings.values` | Live values of every declared setting, kept current by astral |
| `astral.settings.on_change(key, fn)`, `astral.settings.get/set` | Change listeners and persisted values |
| `astral.ui.decorate(Target.MessageAuthor, fn)` | `fn(node)` returns a `Pill`, a list of pills, or plain `{ text, color, colors, text_color, shape }` tables, shown after the guild tag |
| `astral.ui.relabel(target, fn)` | `fn(node)` returns new text; plugins chain. Targets: `MessageAuthor` (authors and reply previews, `node.kind` `NodeKind.Author`/`NodeKind.Reply`), `ChannelName`, `MemberName`, `ComposerPlaceholder`, `ProfileName` (the name on a profile sheet, `node.user_id` from its avatar) |
| `astral.ui.mention(fn)` | `fn(node)` gets each mention chip in a message (`node.text` is the chip's text such as `@name`, `node.index` its position) and returns `{ image, round, hide_at, after }` to draw an image inside the chip, before the name or after it with `after`, and drop the `@` |
| `astral.ui.message_color(fn)` | `fn(node)` returns a colour for a message's text; its alpha says how strongly it blends into each span's own colour. A mention decoration's `color` recolours the whole chip |
| `astral.ui.settings_backdrop({ dim })` | Lays the Nitro or astral theme gradient, darkened by `dim` percent, behind Discord's and astral's settings screens; `nil` removes it |
| `node:message()`, `node:guild()`, `node:member()`, `node:user()`, `node:permissions()`, `node:is_bot()` | Cached lookups for the node a callback receives |
| `astral.discord.guild/member/message/user/nickname/channel_guild/permissions` | Read-only views of Discord's own cache; messages carry `type`, `author`, `reference`, `mentions` (`id`, `username`, `global_name`, `nick`, `avatar`) and `mention_roles`; guild roles carry `icon`, permissions apply channel overwrites |
| `astral.permissions.any/all/combine(granted, Permission.X, ...)` | Permission bit checks |
| `astral.storage.get/set/remove/keys` | The plugin's own data (strings, numbers, booleans, tables), kept in a SQLite database on the device, up to 512 KB per plugin and removed on uninstall |
| `require("lib.util")` | Loads `lib/util.lua` from the plugin's folder once and returns what it returns |

## Permissions

A manifest declares what a plugin reads with `permissions = [...]`; calls outside it raise an error, and the plugin page lists them. `discord.guilds` (`guild`, `channel_guild`), `discord.members` (`member`), `discord.permissions` (`permissions`), `discord.messages` (`message`), `discord.users` (`user`), `discord.relationships` (`nickname`) and `storage` (`astral.storage`).

## Builtins

astral runs a prelude (`packages/core/lua/prelude.lua`, compiled into astral) before each plugin:

| Builtin | Purpose |
| --- | --- |
| `astral.Target`, `NodeKind`, `Shape`, `MessageType`, `Permission` | Read-only enums; reading a member that does not exist is an error |
| `astral.Color` | `Color.hex("#5865F2")`, `Color.rgb(r, g, b, a)`, `Color.from(value)`, `:opaque()`, `:with_alpha(a)`, `:luminance()`, `Color.contrasting(backgrounds)`, `Color.Black/White/Blurple` |
| `astral.Pill` | `Pill("Mod"):color("#4E7FFF")`, `:gradient(colors)`, `:shaped(Shape.Op)`, `:text_colored(color)`; the text colour defaults to the best contrast |
| `astral.class(name, parent)`, `astral.enum(name, members)` | Classes with `.new`/call construction, `init`, `is` and inheritance; frozen enums |
| `astral.list.map/filter/find/contains/push_unique` | List helpers |

A pill with two or three colours draws Phoenix's native `backgroundGradient`; `Shape.Op` draws Discord's original poster style.

## Replay

`astral vphone capture <dir>` records the ViewWire batches Phoenix sends while you use Discord. `astral replay <plugin folder>... <dir>` then runs those batches through the rewriter on the Mac with the plugins loaded in the same sandbox and prelude, and prints every decoration and relabel they return. Discord's cache is absent there, so `astral.discord` lookups return nothing.

## Autocomplete

`packages/sdk/library/astral.lua` describes the whole API for lua-language-server (the Lua extension in VS Code, Zed, Neovim and others), and `.luarc.json` loads it, so `astral.` completes, enums list their members and typos are flagged. Plugins outside this repository copy `packages/sdk/library` and point `workspace.library` at it. `astral lint` runs the same checks on every plugin.

## Decorations

The ViewWire rewriter tracks `message.author`, `message.avatar`, `message.timestamp` and `navigation.channel` nodes. The author's user id comes from the avatar image's URL (two levels below the avatar node), the channel from the selected channel row or, before the channel list exists, from the message's row in the cache. Pills are created under ids from `0x7b000000`, inserted before the timestamp, replaced when the row renders again and removed with it.

## Discord cache

`Application Support/store/<account>/cache.sqlite` holds JSON in key-value tables whose keys are snowflakes zero-padded to 20 digits: `guilds` (guild), `members` (guild:user), `channels` (guild:channel), `channel_overwrites` (guild:channel), `messages` (channel:message) and `users` (user).

## Backup

Export settings writes `astral-backup.toml` (format 1) through the share sheet: astral's settings, each installed plugin's enabled state and settings, and the marketplace plugins and themes installed. Import settings reads one back from Files, applies it and reinstalls missing marketplace items from the catalog. `astral dev backup export|import` does the same over the development API.

## Reports

astral records MetricKit crashes and hangs (using the ported MetricKit parser, which ignores stacks from other apps in the same host) and plugins that stop after errors, with the running plugins and versions, in `Documents/astral/reports.json`. The astral page lists the latest five; tapping one copies the full report and opens a pre-filled issue in astral-app/astral.

## Settings UI

The astral category in Discord's settings has astral, Plugins, Design, Marketplace and Updates rows, cloned from Discord's own settings rows. Pages are pushed onto the settings navigator, up to four deep. The Plugins page draws Unbound-style cards: plug tile, name, author avatars, settings and overflow buttons, switch, description and version tag, with filter chips and an install button.

## Themes

A theme (TOML) maps ARGB colours to ARGB colours under `[colors]` (gradient stops included), can replace the stops of Discord's own background gradients, which Nitro app themes draw with, using `[background] gradient = ["#RRGGBB", ...]`, and can scale corner radii (`[radius] scale`) and font sizes and line heights (`[font] scale`). The rewriter applies all of it in place in each batch before Discord's host decodes it, so Discord draws its own views with the theme; astral's own cards are built separately and keep their sizes.

"Import Unbound theme" on the Design page (or `astral dev theme import`) converts an Unbound theme's `raw` and `semanticColors` overrides into colour mappings. Raw names resolve through Discord's raw palette (`packages/themes/palettes/discord-raw.toml`) and semantic tokens through Discord's semantic table (`packages/themes/palettes/discord-semantic.toml`, from `astral palette semantic`), using each token's dark-theme raw colour and opacity, which is what Phoenix sends. Overrides apply wherever Phoenix draws with those values; Nitro app themes paint their own gradient backgrounds, which stay.
