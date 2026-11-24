# Plugins

Plugins are Lua 5.4 files. astral runs no native plugin code, so a plugin works the same on jailbroken and sideloaded installs and cannot crash Discord.

## Translations

A plugin's strings live in `i18n/<locale>.toml` (flat `KEY = "text"` tables); astral finds them by scanning the folder. `astral.i18n.t(key, { name = value })` returns the string for the best of the device's preferred languages, falling back per key to English and then to the key; `astral.i18n.locale` names the table in use. Translations are part of the plugin's signature, and marketplace installs download and verify them. The two first-party plugins carry Unbound's 32 locales.

## Icons

A manifest's `iconSvg` carries the plugin's icon, as Unbound's catalog does; astral rasterises it on device into a template image Phoenix tints like its own icons. Without one, `icon` may name a Discord icon (`TextIcon`), drawn from Phoenix's `ic_mana_*` assets, and otherwise the card shows the plug. astral's own icons (`assets/*.svg`) are drawn the same way.

## Authors

Cards show each author's avatar the way Unbound does: Discord's local user cache first, then a `GET /users/{id}` with the signed-in session, cached in `Documents/astral/authors.json` for a week, then Discord's default avatar by the author's position.

## Naming

Locals, parameters, functions and fields are `snake_case`, and constants are `UPPER_SNAKE`. `PascalCase` is only for classes and enums, and for their members (`astral.Color.Blurple`, `astral.MessageType.Reply`). `astral check` enforces this.

Plugin settings take an optional `icon` (a Discord icon name such as `PaintPaletteIcon`, or a `res://` URI); rows without one show no icon. `astral.settings.color` stores `#RRGGBB` or `#AARRGGBB`, or an empty string for automatic, and opens a swatch page with a hex field.

## Package

```
packages/addons/plugins/<name>/manifest.toml   {"id", "name", "description", "authors": [{"name", "id"}], "version", "main": "main.lua"}
packages/addons/plugins/<name>/main.lua        at most 256 KB
packages/addons/plugins/<name>/**/*.lua        modules, loaded with require("lib.util"), at most 1 MB together
packages/addons/plugins/<name>/tests/*.lua     run by astral test against a mocked `astral` table and the prelude
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
| `astral.settings.option(value, title, subtitle)` | One choice option; a setting can contain up to 64 options |
| `astral.settings.number(key, title, { min, max, step, default, subtitle, section })` | A number drawn as Phoenix's slider row; returns the current value |
| `astral.settings.text(key, title, default, { max_length, subtitle, section })` | A multiline value edited in a native sheet; text length is limited to 16 KiB by default |
| `astral.settings.values` | Live values of every declared setting, kept current by astral |
| `astral.settings.on_change(key, fn)`, `astral.settings.get/set` | Change listeners and persisted values |
| `astral.audio.speak(text, { language, rate })` | Reads up to 4 KiB of text aloud with the system voice; `language` is a BCP 47 tag and `rate` runs from 0 (slowest) to 1. Requires `audio` |
| `astral.audio.play(file, { volume })`, `astral.audio.stop()` | Plays an mp3, m4a, wav, caf or aac file from the plugin's folder at `volume` 0 to 1, replacing the sound already playing; `stop` ends speech and sound. Requires `audio` |
| `astral.time.now_ms()` | Current Unix time in milliseconds |
| `astral.time.after(ms, fn)`, `astral.time.every(ms, fn)`, `astral.time.cancel(timer)` | Runs `fn` once after `ms` milliseconds (up to a day), or every `ms` milliseconds (at least 16); returns a timer to cancel. A plugin keeps at most 64 timers, and stopping it cancels them |
| `astral.ui.decorate(Target.MessageAuthor, fn)` | `fn(node)` returns a `Pill`, a list of pills, or plain `{ text, color, colors, text_color, shape }` tables, shown after the guild tag. A pill with `icon` draws a 16-point image instead: a `res://` or `https://` URI, or a PNG or WebP up to 64 KiB in the plugin folder; `tint` colours it and `text` becomes its accessibility label |
| `astral.ui.relabel(target, fn)` | `fn(node)` returns new text; plugins chain. Targets: `MessageAuthor` (authors and reply previews, `node.kind` `NodeKind.Author`/`NodeKind.Reply`), `ChannelName`, `MemberName`, `ComposerPlaceholder`, `ProfileName` (the name on a profile sheet, `node.user_id` from its avatar) |
| `astral.ui.mention(fn)` | `fn(node)` gets each mention chip in a message (`node.text` is the chip's text such as `@name`, `node.index` its position) and returns `{ image, round, hide_at, after }` to draw an image inside the chip, before the name or after it with `after`, and drop the `@` |
| `astral.ui.author_color(fn)` | `fn(node)` returns a colour for a message author's name in chat; the node carries `message_id`, `author_id`, `channel_id` and, in servers, `guild_id` |
| `astral.ui.message_color(fn)` | `fn(node)` returns a colour for a message's text; its alpha says how strongly it blends into each span's own colour. With `discord.messages`, `node.reply_to_me` indicates that the cached message replies to a cached message authored by you. A mention decoration's `color` recolours the whole chip |
| `astral.ui.message_background(fn)` | `fn(node)` returns `{ color, radius, bubble }` for a Phoenix message row (with `bubble = true` the color sits behind the message content as a bubble sized to it, otherwise behind the whole row); the node carries message, author and channel IDs when Phoenix exposes them. Colors accept `astral.Color`, an ARGB integer or a hex string; radius is 0 to 32 points. Requires `ui.message_background` |
| `astral.ui.message_action(label, fn, { icon })` | Adds a row at the top of Discord's message long-press menu; a tap closes the menu and calls `fn(node)` with message and channel IDs. `icon` is a `res://` icon and defaults to a puzzle piece. Requires `ui.message_actions`; reading a cached message requires `discord.messages`, and reading its text also requires `discord.messages.content` |
| `astral.ui.name_color(fn)` | `fn(node)` returns a colour for a name: `node.kind` is `header` for member list role headers ("Dev — 1"), `voter` for poll voters, `typing` for names in the typing indicator (matched to members by name), or `avatar` for a name beside a member's avatar such as reaction users; `node.user_id` names the member |
| `astral.ui.typing(fn)` | `fn(node)` gets each name in the typing indicator, with `node.user_id` matched through Discord's cache, and returns `{ image, round }` to draw before the name |
| `astral.ui.reaction(fn)` | `fn(node)` gets each reaction pill under a message (`node.emoji`, `node.burst`, `node.count`, `node.message_id`) and returns `{ images, total }` to draw up to five overlapping faces after the count, with a `+N` badge when `total` is larger; return `nil` until the faces are known and astral asks again |
| `astral.ui.settings_backdrop({ dim })` | Lays the Nitro or astral theme gradient, darkened by `dim` percent (0 by default), behind Discord's and astral's settings screens; `nil` removes it |
| `astral.ui.alert(title, message)` | Shows a native alert after the plugin declares `ui.dialogs` |
| `astral.ui.toast(title, message)` | Shows a brief native notification after the plugin declares `ui.toasts` |
| `astral.ui.web_auth(url, callback_url, callback)` | Opens one in-app HTTPS sign-in flow at a time and calls back with its matching URL, cookies changed during that flow and error after the plugin declares `ui.web_auth`; the callback must use the authorization URL's host |
| `astral.ui.cancel_web_auth(request)` | Cancels a pending in-app sign-in flow after the plugin declares `ui.web_auth` |
| `astral.ui.copy(value)` | Copies up to 16 KiB of text to the system clipboard after the plugin declares `ui.clipboard` |
| `astral.ui.paste()` | Returns up to 16 KiB of text from the system clipboard, or `nil`, after the plugin declares `ui.clipboard`; iOS may ask the user to allow the paste |
| `astral.ui.notify(title, body, { delay })` | Posts a system notification now or after `delay` seconds (up to a week) under `ui.notifications`. iOS schedules it with the system; Android posts it after the delay while Discord is running |
| `astral.ui.open(url)` | Opens an `https://` or `discord://` link, which Discord handles itself when it can |
| `astral.ui.composer(fn)` | Passes unsent composer text to `fn(text)`, which returns up to 128 one-based UTF-8 byte ranges per plugin using `bold`, `italic`, `underline`, `strike`, `code`, `spoiler`, `marker`, `heading1` to `heading3`, `subtext` or `quote`; input is limited to 8 KiB and requires `ui.composer` |
| `astral.ui.haptic(strength)` | Triggers `light`, `medium` or `heavy` native haptic feedback after the plugin declares `ui.haptics` |
| `astral.files.pick(extensions, callback)`, `astral.files.cancel(request)` | Opens the system document picker after the plugin declares `files.read`; the callback receives `{ name, data }` or `nil, reason`, and file contents are limited to 1 MiB |
| `astral.events.on(event, fn)`, `astral.events.off(subscription)` | Subscribes to `app.foreground` or `app.background` with `events.lifecycle`, metadata-only `message.create` events with `events.messages`, or incoming `message.filter` events with `events.messages.filter`; a filter callback returning `true` prevents that new message from reaching Phoenix state |
| `astral.messages.set_text(channel_id, message_id, text, { note })` | Shows `text` in place of a message's content on this device, followed by a dimmed `(note)`; `nil` restores the original. Requires `messages.display` |
| `astral.messages.get_text(message_id)` | Returns the text set with `set_text`, or `nil`. Requires `messages.display` |
| `astral.messages.transform(fn)` | Transforms outgoing message content before Discord sends or edits it; `fn(content, { channel_id, operation })` returns new content or `nil` after declaring `discord.messages.send`. On Android the text is rewritten when the send button is pressed, so edits also report `operation = "send"` |
| `node:message()`, `node:guild()`, `node:member()`, `node:user()`, `node:permissions()`, `node:is_bot()` | Cached lookups for the node a callback receives |
| `astral.discord.reactors(channel, message, emoji, burst, count)` | The first five people who reacted with `emoji`, fetched once per count and `nil` until they arrive |
| `astral.discord.guild/member/message/user/nickname/channel_guild/permissions` | Read-only views of Discord's own cache; messages carry `type`, `author`, `reference`, `mentions` (`id`, `username`, `global_name`, `nick`, `avatar`) and `mention_roles`; guild roles carry `icon`, permissions apply channel overwrites |
| `astral.permissions.any/all/combine(granted, Permission.X, ...)` | Permission bit checks |
| `astral.storage.get/set/remove/keys` | The plugin's own data (strings, numbers, booleans, tables), kept in a SQLite database on the device, up to 512 KB per plugin and removed on uninstall |
| `require("lib.util")` | Loads `lib/util.lua` from the plugin's folder once and returns what it returns |

## Permissions

A `message.create` event contains `message_id`, `channel_id`, `author_id`, `from_me`, `mentioned`, `direct` and `open`; it never includes message content or raw Discord payloads. `mentioned` currently covers direct user mentions. A synchronous `message.filter` callback receives `message_id`, `channel_id`, `author_id`, `referenced_author_id`, `content`, `from_me`, `direct` and `open` before an incoming `MESSAGE_CREATE` reaches Phoenix; returning boolean `true` suppresses that new message. It cannot filter edits or messages already loaded into the chat. Role and everyone mention suppression preferences are not exposed.

A manifest declares what a plugin reads with `permissions = [...]`; calls outside it raise an error, and the plugin page lists them. `discord.guilds` (`guild`, `channel_guild`), `discord.members` (`member`), `discord.permissions` (`permissions`), `discord.messages` (`message` metadata, `reactors`, `node.reply_to_me`), `discord.messages.content` (cached message text returned by `message`), `discord.users` (`user`), `discord.relationships` (`nickname`), `storage` (`astral.storage`), `network` (`astral.net.request`; valid JSON response bodies up to 64 KiB are parsed into `response.json`), `ui.dialogs` (`astral.ui.alert`), `ui.toasts` (`astral.ui.toast`), `ui.notifications` (`astral.ui.notify`), `audio` (`astral.audio`), `ui.web_auth` (in-app HTTPS sign-in), `ui.clipboard` (`astral.ui.copy`, `astral.ui.paste`), `ui.composer` (`astral.ui.composer`), `ui.message_background` (`astral.ui.message_background`), `ui.haptics` (`astral.ui.haptic`), `ui.message_actions` (`astral.ui.message_action`), `messages.display` (`astral.messages.set_text`, `get_text`), `files.read` (`astral.files`), `events.lifecycle` (`app.foreground`, `app.background`), `events.messages` (`message.create`) and `events.messages.filter` (`message.filter` plus incoming message content). File selection uses the platform's system picker and reads only the file the user selects.

```lua
astral.files.pick({ "json" }, function(file, reason)
	if not file then
		astral.log("File selection ended:", reason)
		return
	end
	astral.log("Selected:", file.name)
end)
```

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

Export settings writes `astral-backup.toml` (format 1) through the share sheet: astral's settings, each installed plugin's enabled state and settings, and the marketplace addons installed. Import settings reads one back from Files, applies it and reinstalls missing marketplace items from the catalog. `astral dev backup export|import` does the same over the development API.

## Reports

Reports use schema 2 with platform, install mode, client version, architecture, locale and running plugin IDs. Plugin error details redact token-shaped strings, authorization values, URL query strings and common private paths. Older reports are scrubbed when loaded. Reports do not intentionally include account tokens, message contents or private files. iOS records MetricKit crashes and hangs with stacks filtered to the Discord process; Android records process crash and ANR exit details and includes a system trace when Android provides one. The astral page lists the latest five; tapping one copies the report and opens a pre-filled issue in astral-app/astral.

The cross-platform public API currently supports native alerts, clipboard writes, haptics, editable multiline settings, a system file picker, app lifecycle events, metadata-only message-create events, incoming message filtering and outgoing message transforms. Phoenix does not yet have a verified extension point for slash commands, arbitrary Discord event payloads, plugin notifications, background work or font registration, so those APIs are not exposed. Plugin assets are limited to the package's own Lua modules and declared icons; general asset registration is not available.

## Settings UI

The astral category in Discord's settings has astral, Plugins, Design, Marketplace and Updates rows, cloned from Discord's own settings rows. Pages are pushed onto the settings navigator, up to four deep. The Plugins page draws Unbound-style cards: plug tile, name, author avatars, settings and overflow buttons, switch, description and version tag, with filter chips and an install button.

## Themes

A theme (TOML) maps ARGB colours to ARGB colours under `[colors]` (gradient stops included), can replace the stops of Discord's own background gradients, which Nitro app themes draw with, using `[background] gradient = ["#RRGGBB", ...]`, and can scale corner radii (`[radius] scale`) and font sizes and line heights (`[font] scale`). The rewriter applies all of it in place in each batch before Discord's host decodes it, so Discord draws its own views with the theme; astral's own cards are built separately and keep their sizes.

"Import Unbound theme" on the Design page (or `astral dev theme import`) converts an Unbound theme's `raw` and `semanticColors` overrides into colour mappings. Raw names resolve through Discord's raw palette (`packages/addons/themes/palettes/discord-raw.toml`) and semantic tokens through Discord's semantic table (`packages/addons/themes/palettes/discord-semantic.toml`, from `astral palette semantic`), using each token's dark-theme raw colour and opacity, which is what Phoenix sends. Overrides apply wherever Phoenix draws with those values; Nitro app themes paint their own gradient backgrounds, which stay.
