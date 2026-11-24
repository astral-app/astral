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
| Mention Haptics | Taps for direct mentions and incoming direct messages, with strength and channel settings | `astral.events`, `astral.ui.haptic` |
| Enhanced Blocking | Filters new messages by user ID, literal keywords or a known reply author before Phoenix handles them | `astral.events.on("message.filter")`, `astral.settings.text` |
| Highlight Replies | Tints the text of messages that reply to your messages; row background styling is available but not integrated into this plugin | `astral.ui.message_color`, `node.reply_to_me` |
| Chat Bubbles | Adds rounded color cards behind Phoenix message rows; fit-to-content bubbles and group shaping are not included | `astral.ui.message_background` |
| Translate | Uses the Unbound Translate API when signed in; Automatic otherwise tries Google then MyMemory. A message menu action shows the translation in place; tapping it again restores the original. | `astral.ui.message_action`, `astral.messages.set_text`, `astral.discord.message`, `astral.net.request`, `astral.ui.web_auth`, `astral.ui.toast` |
| Live Markdown | Styles markdown markers and content as they are typed in the composer without changing the message sent | `astral.ui.composer` |

All fifteen add-ons are signed Lua packages with sandboxed tests. Mention Haptics currently recognizes direct user mentions; role/everyone suppression preferences and live Phoenix acceptance remain open. Enhanced Blocking does not filter edits or messages already loaded in chat, does not read Discord's blocked/ignored relationship state, and matches at most the first 64 keywords (128 characters each). Highlight Replies tints message text when the replied-to message is still in cache; row background styling is now available through the shared API but is not wired into that plugin. Chat Bubbles offers rounded full-row cards with selectable colors and corner radius; it does not implement Unbound's fit-to-content geometry, message grouping, media corners or touch behavior. Translate follows Unbound's copy-and-toast default and supports its account-backed API, Discord OAuth callback, expiry-aware token refresh and 401 retry, plus Google/MyMemory fallback. Anonymous providers are limited to 400 bytes; the Unbound API accepts up to 16,000 bytes. Inline translation and the long-press action-sheet entry are not available in Phoenix's current shared UI API, so astral exposes a message action beside the author and does not claim those behaviors. Live Markdown paints supported styles in Phoenix's native composer while leaving the source text unchanged; live UI acceptance remains pending. These are the current cross-platform portability set; the list does not claim full Unbound feature parity.

## User-selected shortlist

The user-selected shortlist had nine Unbound add-ons. Clear URLs, Text Replace, Mention Haptics, Enhanced Blocking, Highlight Replies, Translate and Live Markdown are ported. Chat Bubbles has a first full-row card mode; Expression Tools remains unported. Their remaining native seams are grouped below.

| Candidate | Existing Astral fit | Required Phoenix/API seam | Port group |
| --- | --- | --- | --- |
| Expression Tools | Existing settings pages provide a place for basic preferences | Expression and sticker picker actions, contextual sheets, clone/send operations and Phoenix expression data | 3 |
| Chat Bubbles full parity | Shared `message-row` identifiers support full-row color and radius updates | Fit-to-content geometry, grouped-cell drawing, media corners and touch/scroll behavior across iOS and Android | 4 |

## Porting sequence

1. The shared pre-send transform is implemented for buffered JSON message-create and message-edit requests on iOS and Android. It changes only `content` and preserves other JSON fields; live Phoenix transport coverage remains unverified without authorized app artifacts. Clear URLs and Text Replace use it. Text Replace supports editable plain-text rules and Lua patterns, imports `.txt` files and copies its rules to the clipboard; its pattern syntax differs from JavaScript regular expressions.
2. Highlight Replies uses cached message references and current-user identity to tint reply text when the target message is available. Phoenix row background highlighting remains a separate UI seam. Enhanced Blocking filters incoming `MESSAGE_CREATE` events by user ID, literal keyword and available reply author; filtering edits, already loaded rows and blocked/ignored relationship state remains open. Mention Haptics uses a metadata-only message-create event and permission-gated haptics on both platforms; role/everyone suppression settings remain open.
3. Message actions, bounded JSON responses, in-app OAuth and native toast/clipboard APIs support Translate through a user-tapped action. Its Unbound API session is refreshed for requests and a 401 retries once. Inline message replacement and long-press action-sheet integration remain unsupported. Treat Expression Tools as a separate follow-up because it also needs expression picker and send operations.
4. `astral.ui.composer` provides bounded UTF-8 style ranges with iOS and Android composer adapters for Live Markdown. `astral.ui.message_background` styles the shared Phoenix `message-row` node with bounded color and radius values; Chat Bubbles uses this for full-row cards. Verify both in Phoenix before adding grouped geometry, media shaping or cell-level touch behavior; keep those capabilities in shared APIs with platform adapters.

Do not ship placeholder plugins that only expose settings or transform data without a Phoenix callback that consumes the result. When a candidate needs a missing capability, implement that capability in astral's shared API and both native adapters first. Keep plugin packages Lua-only, add mocked Lua tests, sign them with `astral sign`, and update the catalog. Do not copy Unbound's React Native or Flux implementation into a plugin.
