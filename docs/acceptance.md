# Device acceptance

Results from running astral on real Phoenix builds. Each row names the device, the Discord build and what was seen. Open problems link to the fix that closes them.

## Phoenix access gate, 2026-11-27

A fresh account outside the Phoenix rollout was used in isolated app copies. The account reached the No Access dialog on Android before the corrected evaluators were patched.

| Device and build | Install path | Result |
| --- | --- | --- |
| vphone, iOS 27.2.1, Discord 357.0 (124951) | Injected dylib | Phoenix opens; astral reports two patched evaluators, ready compatibility and native Phoenix settings |
| vphone, iOS 27.2.1, Discord 357.0 (124951) | Sideload IPA, external astral tweak disabled | Phoenix opens; both evaluators are already patched in the IPA; astral reports ready compatibility and native Phoenix settings |
| arm64 AVD, Discord 357.0 alpha (357200) | Vector 2.2, API 102 | Phoenix opens across cold relaunches; astral reports access opened; General shows Phoenix and Lua 5.4 |
| arm64 AVD, Discord 357.0 alpha (357200) | LSPatch 1.2 (487), Vector scope removed | Phoenix opens; astral reports access already open; settings and Plugins render, and General shows Phoenix and Lua 5.4 |

The sideload IPA path was exercised on the vphone with its external astral filter removed; this does not substitute for a separate physical device running stock iOS. App extensions were removed only from the isolated VM installation; the generated IPA retains the required extensions.

The locator validates the experiment descriptor and the two matching evaluator guards. Android needs both immediate and register-offset byte loads, packed ELF relocations and anonymous executable replacement pages for runtime patching. Writing the original file-backed pages prevents execute permission from being restored on the AVD. The Android library links the NDK compiler runtime so cache invalidation resolves at load time.

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

## Android: AVD, Discord Phoenix, LSPosed module

| Area | Result |
| --- | --- |
| Plugin host | All 15 bundled plugins start; Phoenix stays the client across relaunches |
| Telemetry filter | `/api/v9/science` requests are blocked |
| Clear URLs | Did not work: the network rewrite depended on obfuscated OkHttp class names. Outgoing text is now rewritten at the send itself: astral checks the composer's last typed text against the value Phoenix echoes, transforms it when the send button is pressed and delivers one corrected text change first. Sends and edits are cleaned |
| Updates | The catalog refreshes at launch and, at most every 6 hours, when Discord returns to the foreground. Pending addon updates install on their own unless "Update automatically" is off; a newer astral in the catalog shows one notice per version. The repository catalog had announced astral 0.0.3 while builds are 0.0.1; `astral test` now checks it against `version.txt` |
| Crash reports | Android never recorded crashes: the exit reader ran before Discord's Application existed and gave up. It now runs on the first MainActivity resume. That exposed a crash loop (a native tombstone read as text, modified UTF-8 from JNI, and a JSON dump that threw), fixed before merging. A crash-safe plugin trace now names the plugin that was mid-call: a simulated crash during a Translate call reports "Discord process crashed in astral.translate" with the eight running plugins. iOS reports an unexpected exit during a plugin call the same way |
| Staff DevTools | Staff Settings, Show Dev Tools and Build Overrides appear in Discord's settings as soon as it is turned on, and leave when it is turned off |
| Display cutout | The logo was drawn over Android's padded cutout rectangle, larger than the hole; it now follows the cutout path, so it hides behind the camera and only shows in screenshots |
| Plugin message actions | Long presses arrive on the message list as `rowPressDown` with a data index; astral tracks each list's rendered window (`first` plus children) to find the message. Rows look like Discord's; a tap closes the sheet through the activity's back handling |
| Translate | Same as iOS: in place with a "(translated)" note, restored on a second tap |
| Plugins page | Author avatars and plugin icons show. Chat Bubbles, Highlight Replies and Translate named icons Phoenix lacks; they now use `ChatDotsIcon`, `ArrowAngleLeftUpIcon` and `LanguageIcon` |
| Marketplace and row subtitles | The catalog loads; rows show the plugin's icon. Subtitles were cut off and wrapped early because they inherited `flex: 1` from Discord's title text, which split the column's height; they now size to their text at Discord's 12 / 16 subtitle size |

## Android: Discord 357.0 alpha (357200), upgraded in place

| Area | Result |
| --- | --- |
| Gateway messages | Broke silently: the hook named the listener class (`u6`) and its method (`m`); in 357 `u6` is an unrelated class, so message filters, gateway events and the Staff DevTools READY patch stopped. The listener is now found when Discord opens its WebSocket: the client's opener is matched by its parameter types and the listener's text callback by `(socket, String)` |
| Emoji packs in the composer | Broke silently: the composer class `ej` became an unrelated class. Phoenix inputs are now recognised by package instead of a class name |
| Shaded OkHttp | Discord's existing-client field moved; that patch was redundant (every client is built after the constructor hook) and is gone. Request, URL, method and response fields are found by type, each with exactly one match or an error |
| Compatibility scan | `android-compat` now checks every obfuscated class astral still names, from `PhoenixClasses.kt` |
| Settings and menu text | Blank: 357 wraps each row label in a column view, and astral took the row content's second child as the label. Labels are now the first `text` node under that slot, however deeply Discord wraps it |
| Shaded OkHttp crash | Matching the response code field by type found ART's hidden `Object.shadow$_monitor_` too; the lookup threw inside Discord's network thread. Field matching now stops at `Object`, the telemetry path is resolved and checked once at install, and Staff DevTools rewrites log and pass the original through on failure |
| Staff DevTools | Broke in 357 (the URL builder class moved); gateway compression is now removed by editing the URL's own address and query fields, found by value |

## Compatibility scans, 2026-11-24

| Build | Result |
| --- | --- |
| Android stable 354012 | No Phoenix engine yet |
| Android beta 356108 | No Phoenix engine yet |
| Android alpha 356207 (the AVD) | Supported, 77 / 77 properties |
| Android alpha 357200 | Supported, 77 / 77 properties, every hook found |

## Tooling added for acceptance

- `astral vphone open` and `astral avd open` open Discord links (Android takes them as `https://discord.com/…`): `user <id>`, `channel <guild> <channel> [message]`, `dm [channel]`, `settings [section]` or any URL. Settings sections are Phoenix's `settings.row.<name>` ids, such as `account`.
- `astral vphone capture` also saves astral's rewritten batches as `NNNN.bin.out` beside each original.
- `astral avd` boots, deploys, opens, taps by text or id, and reads logs on the Android emulator.
