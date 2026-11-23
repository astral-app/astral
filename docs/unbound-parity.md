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

## User-selected shortlist

These nine Unbound add-ons are the preferred candidates for the next portability pass. The ordering below groups features by the Phoenix seams they share; it is not a claim that they already work with today's API.

| Candidate | Existing Astral fit | Required Phoenix/API seam | Port group |
| --- | --- | --- | --- |
| Clear URLs | Lua can implement the tracking-parameter rules | A shared pre-send transform for both sends and edits, with safe handling of attachments and non-text message fields | 1 |
| Text Replace | Lua can implement literal and pattern-based replacements | The same pre-send transform, plus editable rules and import/export UI beyond switch, choice and number settings | 1 |
| Highlight Replies | Message references and `astral.ui.message_color` are available | Current-user identity and a row-level background/highlight target; text tint alone does not match the Unbound behavior | 2 |
| Client Side Block → Enhanced Blocking | Message and member lookups provide some inputs | Pre-store message filtering, row visibility control, relationship state and a manageable user/keyword rules UI | 2 |
| Mention Haptics | `astral.events` already scopes lifecycle subscriptions | Message-create events, mention suppression settings and a permission-gated haptics adapter on both platforms | 2 |
| Translate | `astral.net.request` and plugin storage are available | Contextual message actions, translated-content presentation and provider/language settings | 3 |
| Expression Tools | Existing settings pages provide a place for basic preferences | Expression and sticker picker actions, contextual sheets, clone/send operations and Phoenix expression data | 3 |
| Live Markdown | Composer placeholders can be relabeled | Composer input/rendering control, inline formatting and custom-emoji insertion; Unbound's implementation also has iOS-specific native behavior | 4 |
| Chat Bubbles | Themes can change some existing colors | Message-row geometry, grouped-cell drawing, media corners and touch/scroll behavior across iOS and Android | 4 |

## Porting sequence

1. Define a shared pre-send message transform after confirming Phoenix's send and edit boundaries on both platforms. Keep it constrained to message content and preserve attachments, replies and other payload fields. This seam can support Clear URLs and Text Replace.
2. Add message-row highlight/visibility controls and current-user identity, then port Highlight Replies, Enhanced Blocking and Mention Haptics with explicit event and haptic permissions.
3. Add contextual message actions and a safe translated-content surface before porting Translate. Treat Expression Tools as a separate follow-up because it also needs expression picker and send operations.
4. Design composer rendering and message-cell customization only after Phoenix's native UI lifecycle is mapped independently on iOS and Android. Live Markdown and Chat Bubbles should use shared API contracts with platform adapters, not native plugin code.

Do not ship placeholder plugins that only expose settings or transform data without a Phoenix callback that consumes the result. When a candidate needs a missing capability, implement that capability in astral's shared API and both native adapters first. Keep plugin packages Lua-only, add mocked Lua tests, sign them with `astral sign`, and update the catalog. Do not copy Unbound's React Native or Flux implementation into a plugin.

When a candidate needs a missing capability, implement that capability in astral's shared API and both native adapters first. Keep plugin packages Lua-only, add mocked Lua tests, sign them with `astral sign`, and update the catalog. Do not copy Unbound's React Native or Flux implementation into a plugin.
