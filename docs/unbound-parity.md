# Unbound add-on parity

astral ports Unbound behavior through Phoenix's shared Lua API. It does not carry over React Native components, Metro hooks, Flux stores or native plugin code.

## Ported Lua add-ons

| astral add-on | Unbound behavior | astral API surface |
| --- | --- | --- |
| Mention Avatars | Shows avatars and role icons in mention chips | `astral.ui.mention`, `astral.discord` |
| More User Tags | Adds webhook, moderator and role tags | `astral.ui.decorate`, `astral.discord`, `astral.permissions` |
| Reaction Avatars | Shows reactors beside reaction pills | `astral.ui.reaction`, `astral.discord.reactors` |
| Role Color Everywhere | Colors mentions, message text, role headers and avatar names | `astral.ui.mention`, `message_color`, `name_color`, `astral.discord` |
| Settings Theme | Shows the active theme behind settings | `astral.ui.settings_backdrop` |
| Show Me Your Name | Adds usernames and friend nicknames to message authors | `astral.ui.relabel`, `astral.discord` |
| Typing Avatars | Shows avatars beside typing names | `astral.ui.typing`, `astral.discord` |

All seven are Lua packages with sandboxed tests and signed manifests. These are the current cross-platform portability set; the list does not claim full Unbound feature parity.

## Candidates with missing Phoenix support

| Unbound add-on | Existing fit | Missing surface |
| --- | --- | --- |
| Clean Channel Names | `astral.ui.relabel(Target.ChannelName, fn)` can replace a channel label | The callback has no channel type, and the Lua runtime has no Unicode NFKC normalization. Both are needed to preserve voice separators and normalize compatibility glyphs consistently. |
| Message Colors | `astral.ui.message_color(fn)` can tint a whole message | Unbound parses individual text nodes and renders inline color pills. astral currently exposes only whole-message tinting. |
| Friends Since | `astral.ui.decorate` can add a pill beside supported labels | There is no profile-sheet decoration target or relationship creation timestamp in the read-only cache API. |
| Show Roles in Chat | `More User Tags` and `Role Color Everywhere` cover selected role metadata | A parity port needs confirmed Phoenix data for the requested role list and a supported label or decoration target at that location. |

## Porting order

1. Add channel type to channel-label callbacks and expose cross-platform NFKC normalization in the Lua API; then port Clean Channel Names with cases for voice separators, decorative glyphs, emoji-only names and empty results.
2. Add a constrained inline text decoration API only after the Phoenix wire schema for text spans is confirmed on both platforms; then port Message Colors with parsing tests and strict text-node bounds.
3. Add a profile-sheet decoration target and the minimum account creation metadata needed for Friends Since; keep relationship data read-only.
4. Recheck Show Roles in Chat against live Phoenix wire data before adding a new target or cache field.

When a candidate needs a missing capability, implement that capability in astral's shared API and both native adapters first. Keep plugin packages Lua-only, add mocked Lua tests, sign them with `astral sign`, and update the catalog. Do not copy Unbound's React Native or Flux implementation into a plugin.
