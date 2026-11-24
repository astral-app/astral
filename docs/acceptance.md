# Device acceptance

Results from running astral on real Phoenix builds. Each row names the device, the Discord build and what was seen. Open problems link to the fix that closes them.

## iOS: vphone, Discord 356.0 (124376), jailbroken tweak install

| Area | Result |
| --- | --- |
| Plugin host | All 15 bundled plugins start with no errors after local development copies were removed |
| Settings | astral's section, General, Plugins, Design, Marketplace and Updates pages render and navigate; back works |
| Display cutout | astral's logo shows in the Dynamic Island |
| Mention Avatars, Role Color Everywhere | Mention chips show the avatar and role colour |
| Reaction Avatars | Faces show on every reaction pill, including custom emoji |
| More User Tags | Owner pill shows |
| Typing Avatars | Avatar and role colour show in the typing line (local injected typing event) |
| Emoji packs | Fluent emoji show in messages and reactions |
| Telemetry filter | Requests are blocked and counted |
| Clear URLs | Works after the network filter learned to read streamed request bodies; Phoenix sends message JSON as an `HTTPBodyStream` |
| Live Markdown | Works after composer detection moved from React Native's `DCDChatInputComponentView` to Phoenix's `chat-input-container` |
| Show Me Your Name | Works on newly drawn rows, but the label is cut off by the crowded header |
| Translate | A row at the top of the long-press menu, styled like Discord's own; the translation replaces the text in place with a dimmed "(translated)" note, and a second tap restores the original |
| Plugin message actions | Rows join the first group of Discord's message menu with its dividers and corner radii; the menu closes and the action runs on the pressed message |
| Chat Bubbles | The rewriter writes the row background and radius (seen in captured output), but iOS does not draw it; open |
| Plugin settings writes | `astral.settings.set` now updates `astral.settings.values` and calls change listeners |

## Tooling added for acceptance

- `astral vphone open` and `astral avd open` open Discord links: `user <id>`, `channel <guild> <channel> [message]`, `dm [channel]`, `settings [section]` or any URL. Settings sections are Phoenix's `settings.row.<name>` ids, such as `account`.
- `astral vphone capture` also saves astral's rewritten batches as `NNNN.bin.out` beside each original.
- `astral avd` boots, deploys, opens, taps by text or id, and reads logs on the Android emulator.
