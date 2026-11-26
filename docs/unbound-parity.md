# Unbound add-on parity

astral ports Unbound's plugins as signed Lua packages. They behave and look like the Unbound originals but run on Phoenix through the shared `astral` API; no React Native, Metro or Flux code carries over. Plugin APIs stay generic: a plugin builds its feature from primitives such as `ui.patch`, `ui.insert`, `ui.style_text` and `discord.request` rather than from hooks made for it.

## Ported plugins

| Plugin | What it does | astral API it uses |
| --- | --- | --- |
| [Always Animate](../packages/addons/plugins/always-animate/) | Animates avatars, server icons, banners and emoji everywhere, not only when you tap them. | `astral.ui.image_rewrite` |
| [Channel Typing Indicators](../packages/addons/plugins/channel-typing-indicators/) | Shows typing dots beside channels where people are typing. | `astral.discord.relationship`, `astral.events.on`, `astral.ui.insert`, `astral.ui.refresh` |
| [Chat Bubbles](../packages/addons/plugins/chat-bubbles/) | Draws a rounded bubble behind each message's text, media and embeds. | `astral.ui.message_background` |
| [Clear URLs](../packages/addons/plugins/clear-urls/) | Removes common tracking parameters from links before messages are sent or edited. | `astral.messages.transform` |
| [Cross Mod Badges](../packages/addons/plugins/cross-mod-badges/) | Shows the badges people have from other client mods, such as Vencord, Aliucord and BetterDiscord, on their profile. | `astral.net.intercept`, `astral.net.release`, `astral.net.request`, `astral.ui.image_rewrite` |
| [Declutter](../packages/addons/plugins/declutter/) | Hides buttons, upsells and profile decorations you never use: the gift button, Server Boosts, call buttons, avatar decorations, nameplates, server tags and more. | `astral.events.on`, `astral.net.intercept`, `astral.ui.patch` |
| [Enhanced Blocking](../packages/addons/plugins/enhanced-blocking/) | Hides messages by user, keyword or reply author, including people you blocked, edits and messages already loaded. | `astral.discord.message`, `astral.discord.relationship`, `astral.events.on`, `astral.ui.patch` |
| [Expression Tools](../packages/addons/plugins/expression-tools/) | Useful tools for custom emojis. | `astral.discord.emoji`, `astral.discord.guilds`, `astral.discord.me`, `astral.discord.permissions`, `astral.media.offer_pick`, `astral.net.request`, `astral.ui.copy`, `astral.ui.dismiss`, `astral.ui.insert`, `astral.ui.open`, `astral.ui.patch`, `astral.ui.press`, `astral.ui.sheet`, `astral.ui.toast` |
| [File Size On Picker](../packages/addons/plugins/file-size-on-picker/) | Shows file sizes on media picker thumbnails. | `astral.media.picked`, `astral.ui.insert` |
| [Fix GIF Downloads](../packages/addons/plugins/fix-gif-download/) | Save Discord GIFs as GIF files instead of videos. | `astral.discord.raw_message`, `astral.media.on_save`, `astral.net.request` |
| [Force Owner Crown](../packages/addons/plugins/force-owner-crown/) | Always show the owner crown on the server owner's messages. | `astral.ui.decorate` |
| [GIF Paste](../packages/addons/plugins/gif-paste/) | Makes GIF picker selections paste a link into the chat box instead of sending immediately. | `astral.gateway.dispatch`, `astral.net.intercept`, `astral.ui.insert_text` |
| [GIF Provider Switcher](../packages/addons/plugins/gif-provider-switcher/) | Searches GIFs from Tenor or Giphy instead of Klipy in Discord's own GIF picker. | `astral.net.intercept`, `astral.net.release`, `astral.net.request`, `astral.net.respond` |
| [Highlight Replies](../packages/addons/plugins/highlight-replies/) | Tints the text of messages that reply to your messages. | `astral.ui.style_text` |
| [IRC Colors](../packages/addons/plugins/irc-colors/) | Gives everyone in chat their own username color, picked from their user id like in IRC clients. | `astral.discord.channel_guild`, `astral.discord.guild`, `astral.discord.member`, `astral.ui.patch` |
| [Live Markdown](../packages/addons/plugins/live-markdown/) | Styles Discord markdown as you type in the message composer. | `astral.ui.composer` |
| [Local Notifications](../packages/addons/plugins/local-notifications/) | Notifies you of mentions and DMs on installs where Discord's push notifications don't arrive, by keeping Discord connected in the background. | `astral.background.keep_alive`, `astral.discord.guild`, `astral.events.on`, `astral.ui.notify` |
| [Mention Avatars](../packages/addons/plugins/mention-avatars/) | Shows user avatars and role icons inside mentions. | `astral.ui.style_text` |
| [Mention Haptics](../packages/addons/plugins/mention-haptics/) | Taps your phone when a new message mentions you, your roles or everyone, or arrives in a direct message. | `astral.events.on`, `astral.ui.haptic` |
| [Message Latency](../packages/addons/plugins/message-latency/) | Marks messages that took at least the selected time to send, beside their timestamp. | `astral.events.on`, `astral.ui.insert`, `astral.ui.refresh` |
| [Message Link Embeds](../packages/addons/plugins/message-link-embeds/) | Adds a preview under messages that link to other Discord messages, and opens the linked message when tapped. | `astral.discord.message`, `astral.discord.raw_message`, `astral.discord.user`, `astral.ui.insert`, `astral.ui.open_channel`, `astral.ui.refresh` |
| [Message Logger](../packages/addons/plugins/message-logger/) | Keeps deleted messages in the chat in red and shows what edited messages said before. | `astral.discord.message`, `astral.events.on`, `astral.gateway.dispatch`, `astral.ui.message_action`, `astral.ui.refresh`, `astral.ui.text_transform` |
| [Message Text Size](../packages/addons/plugins/message-text-size/) | Makes the text of chat messages bigger or smaller, leaving the rest of Discord as it is. | `astral.ui.patch` |
| [More User Tags](../packages/addons/plugins/more-user-tags/) | Adds tags for webhooks and moderative roles, and can show each member's top coloured role. | `astral.ui.decorate` |
| [New Member Sprout](../packages/addons/plugins/new-member-sprout/) | Shows desktop's new member sprout beside people who joined the server in the last week. | `astral.ui.decorate` |
| [New User Sprout](../packages/addons/plugins/new-user-sprout/) | Shows a sprout beside people whose Discord account is less than the chosen number of days old. | `astral.ui.decorate` |
| [No Reply Mention](../packages/addons/plugins/no-reply-mention/) | Turns off the reply ping by default, or only pings the people you list. | `astral.discord.message`, `astral.net.intercept` |
| [Picker Video Previews](../packages/addons/plugins/picker-video-previews/) | Plays videos when you long-press them in the media picker, instead of showing a still frame. | `astral.media.picked`, `astral.ui.insert`, `astral.ui.refresh` |
| [Preview Message](../packages/addons/plugins/preview-message/) | Adds a button to the chat input that shows how your message will look before you send it. | `astral.discord.user`, `astral.events.on`, `astral.messages.send_local`, `astral.ui.composer`, `astral.ui.composer_text`, `astral.ui.insert`, `astral.ui.refresh`, `astral.ui.toast` |
| [PronounDB](../packages/addons/plugins/pronoundb/) | Shows people's pronouns from PronounDB beside their name in messages. | `astral.net.request`, `astral.ui.decorate`, `astral.ui.refresh` |
| [Reaction Avatars](../packages/addons/plugins/reaction-avatars/) | Shows the people who reacted beside each message reaction. | `astral.discord.message`, `astral.discord.request`, `astral.events.on`, `astral.ui.insert`, `astral.ui.refresh` |
| [ReviewDB](../packages/addons/plugins/reviewdb/) | Leave reviews on other users' profiles and see what others think of you. | `astral.discord.user`, `astral.events.on`, `astral.net.request`, `astral.ui.confirm`, `astral.ui.insert`, `astral.ui.open`, `astral.ui.sheet`, `astral.ui.toast`, `astral.ui.web_auth` |
| [Role Color Everywhere](../packages/addons/plugins/role-color-everywhere/) | Colors mentions, message text, role headers and names beside avatars with members' role colors. | `astral.discord.member`, `astral.ui.style_text` |
| [Settings Theme](../packages/addons/plugins/settings-theme/) | Shows your theme behind User Settings and every page in it, which Discord leaves plain. | `astral.ui.app_background`, `astral.ui.patch`, `astral.ui.refresh` |
| [Shiki Codeblocks](../packages/addons/plugins/shiki-codeblocks/) | Highlights code blocks with VS Code-style syntax colors. | `astral.code.highlight`, `astral.discord.raw_message`, `astral.net.request`, `astral.ui.code_block`, `astral.ui.copy`, `astral.ui.refresh`, `astral.ui.toast` |
| [Show Me Your Name](../packages/addons/plugins/show-me-your-name/) | Display usernames next to display names, or show usernames only. | `astral.discord.message`, `astral.discord.nickname`, `astral.ui.relabel` |
| [Sticker Preview](../packages/addons/plugins/sticker-preview/) | Displays a preview of a selected sticker instead of sending it immediately. | `astral.gateway.dispatch`, `astral.net.intercept`, `astral.ui.composer_text`, `astral.ui.insert`, `astral.ui.insert_text`, `astral.ui.refresh` |
| [Text Replace](../packages/addons/plugins/text-replace/) | Rewrites outgoing message text with literal or Lua pattern rules. | `astral.files.pick`, `astral.messages.transform`, `astral.ui.alert`, `astral.ui.copy` |
| [Tone Indicators](../packages/addons/plugins/tone-indicators/) | Turns tone tags such as /j and /srs into pills that spell out what they mean. | `astral.ui.text_transform` |
| [Translate](../packages/addons/plugins/translate/) | Translates a message from its long-press menu with Unbound Translate, Google or MyMemory, into a toast or under the message. | `astral.messages.get_text`, `astral.messages.set_text`, `astral.net.request`, `astral.ui.cancel_web_auth`, `astral.ui.copy`, `astral.ui.message_action`, `astral.ui.toast`, `astral.ui.web_auth` |
| [Typing Avatars](../packages/addons/plugins/typing-avatars/) | Shows the avatar of each person typing in front of their name. | `astral.discord.user`, `astral.ui.style_text` |
| [Unsuppress Embeds](../packages/addons/plugins/unsuppress-embeds/) | Adds a message long-press row that brings back suppressed link embeds, or suppresses them. | `astral.discord.me`, `astral.discord.permissions`, `astral.discord.request`, `astral.ui.message_action`, `astral.ui.toast` |
| [View Raw](../packages/addons/plugins/view-raw/) | View and copy raw data for messages. | `astral.discord.raw_message`, `astral.ui.copy`, `astral.ui.message_action`, `astral.ui.sheet`, `astral.ui.toast` |

## Known gaps

- **Message Link Embeds:** the preview lacks the author's avatar, the timestamp and attachments.
- **Chat Bubbles:** grouped corners and media shaping follow Unbound only partly.

## Discord API calls

`astral.discord.request` sends any Discord API call as the signed-in account. Each call copies the last request Discord itself made (its headers and cookies) and goes out through Discord's own client: Phoenix's OkHttp client on Android, the network filter's forwarding session on iOS. astral never reads the account token, and `astral.net.request` refuses Discord's API.

## Porting rules

- Credit the original Unbound, Vencord and Equicord authors before adrian in `manifest.toml`.
- Match Unbound's behaviour and UI one to one; take a screenshot of each port for sign-off.
- Add a missing capability to the shared API, with both platform adapters, before the plugin that needs it.
- Keep plugins Lua-only with mocked Lua tests, sign them with `astral sign` and list them in the catalog.
