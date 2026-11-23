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
| Clear URLs | Removes tracking parameters from links before messages are sent or edited | `astral.messages.transform` |
| Text Replace | Rewrites outgoing messages using editable literal or Lua pattern rules with optional context | `astral.messages.transform`, `astral.settings.text`, `astral.files`, `astral.ui.copy` |

All nine add-ons are signed Lua packages with sandboxed tests. These are the current cross-platform portability set; the list does not claim full Unbound feature parity.

## User-selected shortlist

The user-selected shortlist had nine Unbound add-ons. Clear URLs and Text Replace are ported; the remaining candidates below are grouped by the Phoenix seams they share.

| Candidate | Existing Astral fit | Required Phoenix/API seam | Port group |
| --- | --- | --- | --- |
| Highlight Replies | Message references and `astral.ui.message_color` are available | Current-user identity and a row-level background/highlight target; text tint alone does not match the Unbound behavior | 2 |
| Client Side Block → Enhanced Blocking | Message and member lookups provide some inputs | Pre-store message filtering, row visibility control, relationship state and a manageable user/keyword rules UI | 2 |
| Mention Haptics | `astral.events` already scopes lifecycle subscriptions | Message-create events, mention suppression settings and a permission-gated haptics adapter on both platforms | 2 |
| Translate | `astral.net.request` and plugin storage are available | Contextual message actions, translated-content presentation and provider/language settings | 3 |
| Expression Tools | Existing settings pages provide a place for basic preferences | Expression and sticker picker actions, contextual sheets, clone/send operations and Phoenix expression data | 3 |
| Live Markdown | Composer placeholders can be relabeled | Composer input/rendering control, inline formatting and custom-emoji insertion; Unbound's implementation also has iOS-specific native behavior | 4 |
| Chat Bubbles | Themes can change some existing colors | Message-row geometry, grouped-cell drawing, media corners and touch/scroll behavior across iOS and Android | 4 |

## Porting sequence

1. The shared pre-send transform is implemented for buffered JSON message-create and message-edit requests on iOS and Android. It changes only `content` and preserves other JSON fields; live Phoenix transport coverage remains unverified without authorized app artifacts. Clear URLs and Text Replace use it. Text Replace supports editable plain-text rules and Lua patterns, imports `.txt` files and copies its rules to the clipboard; its pattern syntax differs from JavaScript regular expressions.
2. Add message-row highlight/visibility controls and current-user identity, then port Highlight Replies, Enhanced Blocking and Mention Haptics with explicit event and haptic permissions.
3. Add contextual message actions and a safe translated-content surface before porting Translate. Treat Expression Tools as a separate follow-up because it also needs expression picker and send operations.
4. Design composer rendering and message-cell customization only after Phoenix's native UI lifecycle is mapped independently on iOS and Android. Live Markdown and Chat Bubbles should use shared API contracts with platform adapters, not native plugin code.

Do not ship placeholder plugins that only expose settings or transform data without a Phoenix callback that consumes the result. When a candidate needs a missing capability, implement that capability in astral's shared API and both native adapters first. Keep plugin packages Lua-only, add mocked Lua tests, sign them with `astral sign`, and update the catalog. Do not copy Unbound's React Native or Flux implementation into a plugin.
