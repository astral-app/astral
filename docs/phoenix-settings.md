# Phoenix settings: component and navigation boundary

Host: official `com.hammerandchisel.discord` 356.0 build 124376. Owning image `PhoenixFatClient.framework/PhoenixFatClient`, UUID `381f8c42-ad92-3e30-80a4-9599f0b11b50`, IPA SHA-256 `e6a3fc1e7bc9a6dbdd67217feaee310c43ee2dc1f002fd439ff76811b79c7a7a`. Addresses are image-relative observations for this UUID only, not contracts.

## Why the current entry is not a Phoenix category

Loader 0.4.0 adds a UIKit button to the overview `ScrollBox` and presents its own table in a sheet. Phoenix does not build settings that way. The Rust guest describes the screen and sends ViewWire batches; the Swift host (`LibdiscordHost`) turns each batch into host views (`FlexBox`, `TextBox`, `ImageBox`, `RowCell`, `ScrollBox`, `NavigatorBox`, `PageSheetController`, `SheetController`). A built-in category, its rows and the detail screens it opens are all guest-described nodes. Navigation is guest-owned: a press goes back to the guest as an event (`UIManager.batchEvents`), and the guest answers with ViewWire edits to the navigator's children.

## Host decoder

| Observation | Image-relative address |
| --- | --- |
| Host-call frame dispatcher (`malformed call frame`, `call %s %ld ms`) | `0x3b3c0` |
| `opsArrived(_ data: Data)`, which drops bad batches (`ops batch dropped`) | `0x333ce8` |
| Decoder: version check against `view wire version`, `childEdits without ChildEdits` | `0x7b424` |
| Property-name diagnostics (`ViewWire: \`%@\` is not in prop_names.txt; dropped`) | `0x57fc8`, `0x7afac`, `0x29b98c` |
| Child list cache | Swift class `LibdiscordHost.ViewWire.ChildLists` |

The iOS host resolves properties by name (`ViewWire: \`%@\` is not in prop_names.txt; dropped`), so property IDs follow the same `prop_names` table as Android's 810-entry registry.

## Consequence for the integration

ViewWire op 3 replaces a parent's whole child list and op 10 edits it by keep, drop and insert counts against the host's cached children. Adding nodes to a guest-owned child list behind the decoder's back would make the guest's next op 10 invalid against the cache. A genuine astral category therefore has to go through the decoder path:

1. Create astral nodes with op 1 under a reserved ID range, using the view types and properties the guest gives a built-in category (captured from the live stream, not guessed).
2. Rewrite the guest's op 3 and op 10 for the overview list so the astral span stays at a fixed position and the guest's keep counts still describe its own children.
3. Handle host events for astral IDs locally and keep them out of `UIManager.batchEvents`.
4. Push and pop the astral detail screen with the same navigator child edits the guest uses for a built-in category.

Function boundaries come from `LC_FUNCTION_STARTS`; the call chain is `0x7d01c` (closure) → `0x7cf00` → `0x7cc9c` → `0x3b3c0` → `0x333ce8` → `0x7b424`.

## Live capture (wire probe 0.1.0)

`probes/ios/wire-probe` hooks `opsArrived(_:)` at `0x333ce8` after checking the bundle, the Phoenix UUID and the first four prologue words. A naked trampoline saves `x0`/`x1`, leaves the Swift context in `x20` untouched, and branches to the original. The argument is a `Foundation.Data` (the decoder copies it with `Data.Iterator` and `_copyContents`), bridged with `Data._bridgeToObjectiveC()` and summarised off the main thread. Text values are kept only as length and a 32-bit hash; view types, IDs, child lists, property IDs, value tags and numbers are kept. Output is bounded to 4,000 batches and 48 MB.

On the SE (official 356.0 build 124376, UUID verified on the installed framework), all 17 batches of one session decoded completely, from cold start through opening Settings, opening Account and going back. Batches arrive off the main thread. The session used ops 1, 2, 3, 4, 5, 6, 7, 8 and 9; it used no op 10.

| Step | Batch | Ops |
| --- | --- | --- |
| Settings overview opens | seq 13, 44,817 bytes | 373 creates, 206 child lists |
| Account opens | seq 14, 16,769 bytes | 141 creates, 92 child lists |
| Back to the overview | seq 17, 1,146 bytes | one child list, one removal, one op 9 |

The built-in category shape (`evidence/ios-356/settings-category-structure.json`):

- A group `view` holds the header `text` and a card `view` (corner radius 12).
- The card alternates row views and hairline separators.
- A row `view` carries an accessibility label (property 11), an identifier string (9), a colour or handler integer (90), two word lists (733 and 740, tag 11) that are the likeliest event bindings, and corner radii on the first (391, 392) and last (389, 390) rows.
- Inside a row: an icon `image` (428 is an asset name), the title `text` (499) and a chevron `image`.

Property 499 is text content, 11 an accessibility label and 354 an input placeholder.

## Rendered astral category (wire probe 0.3.0)

Probe 0.2.0 kept identifier strings on non-text properties. The Account row's identifier (property 9) is `settings.row.account`; property 320 lists the events a node subscribes to (`["press"]`); 414 is the accessibility role (`button`, `header`). Icon sources are `res://` asset names from Phoenix's `Assets.car` (`res://ic_mana_user_circle`, chevron `res://ic_mana_chevron_small_right`); the catalogue has `ic_mana_sparkles`. Op 9 carries `(8, slot, f32 bits)` triples, initial values for animated shared values, not node IDs.

Probe 0.3.0 rewrites batches inside the hook and hands the host a new `Data` (`Data._unconditionallyBridgeFromObjectiveC`):

1. Anchor: an op 1 whose property 9 is `settings.row.account`. Its card, category group and the overview list are found from the batch's op 3 lists.
2. Clone: the group, header, card, row, row content, icon, title, chevron container and chevron are re-encoded under IDs from `0x7a000000`. Header and title read `astral`, the row identifier is `settings.row.astral`, the icon is `res://ic_mana_sparkles`, the row gets all four corner radii, and the `press` subscription is dropped so no press reaches the guest for an astral node.
3. Order: the clone and its op 3 lists are appended after the guest's ops. Tag 11 word lists append to a batch-local cache that later tag 12 values index, so inserting earlier would shift the guest's indices.
4. Placement: a final op 3 on the overview list inserts the astral group after the first child (Search).
5. Lifetime: a later op 3 on the overview list is re-applied with the astral group; an op 4 that removes the list removes the nine astral nodes.

On the SE the category rendered with Discord's own row component below Search and above Account Settings. Opening Account and returning kept a single astral category, scrolling worked, and every batch in the session still decoded completely. One batch needed rewriting. The rewriter has a host test under ASan and UBSan.

## Navigation boundary

The settings stack is a `navigator` node whose children are the stack (home, profile, settings overview). Opening Account appends the page root to the navigator's children and sets navigator property 252 (the active index) from 2 to 3; back sets 252 to 2, removes the page subtree with op 4, restores the children and resets shared values with op 9. The page itself carries a 56-point placeholder at the top: the back button and title belong to the host's native navigation bar, which reports a back press to the guest. An astral page pushed with the same ops would therefore send its back press to the guest, which would pop the settings overview. The host-to-guest event path has to be captured before an astral page can be pushed.

Known gap: the rewritten `Data` returned by the bridge is not released after the host consumes it (one allocation per overview open).

## Engine selection

Discord's launcher keeps the engine choice in the `discord_fat_client` defaults suite: `launch_client` (`phoenix` when **Launch Phoenix on next start** is on) and `unhealthy_phoenix_launches`, which makes the launcher fall back to React Native after unhealthy launches. Repeated forced restarts during testing tripped it once. Loader 0.4.1 sets `launch_client` to `phoenix` and resets the counter in its constructor, before `main` and before the launcher reads the suite, and records the previous values in `status.json`. With the suite planted at `react_native` and 5 unhealthy launches, the next cold launch came up in Phoenix and the wire hook installed.

## Press routing

Property 9 becomes the host view's `accessibilityIdentifier`. Built-in rows carry `GuestTapRecognizer`, `HighlightRecognizer` and `PressWatch`; the first two come from the `press` subscription. The astral row without `press` only had `PressWatch`. The loader will keep `press` for the native highlight and retarget the `GuestTapRecognizer` on views whose identifier starts with `settings.row.astral`, so astral taps never reach the guest.

## astral page (wire probe 0.5.1)

Back buttons are guest nodes too: a `view` with identifier `Back`, a `press` subscription and an icon (`res://ic_mana_x_small` on the overview, `res://ic_mana_arrow_large_left` on detail pages). Each page carries its own header bar, so an astral page can carry its own back button. A page is a fixed skeleton: root, `settings-phone`, column, and a container with a 56-point spacer, a hairline, a `scroll` holding the sections, and the header bar (back button and title). The overview has the same skeleton.

The probe now:

1. Stores the overview batch's creates for the skeleton, header, section group, row and separator as templates, found structurally from the anchor row.
2. Tracks every `navigator`. The settings navigator is the one whose children contain the overview root; the first attempt tracked the newest navigator and pushed onto the tab navigator instead.
3. Opens the page from a native tap on the astral row: creates under IDs from `0x7a000100`, op 3 appending the page to the settings navigator, op 2 setting property 252 to the new index. Back sets 252 to the previous index, restores the children and removes the page with op 4. A guest op 3 on the settings navigator while the page is open removes the page as well.
4. Drops tag 12 values from templates, since they index the cache of the batch they came from.
5. Delivers astral batches through the guest's original host-call entry as ops frames. The intercept holds a mutex across the guest's call; astral takes the same mutex on a background queue, so rewriting, page state and decoding never run concurrently and the main thread never waits on the guest.
6. Routes taps by retargeting the `GuestTapRecognizer` of views whose identifier is `settings.row.astral…` or `astral.back`, and turns off the navigator's interactive back swipe while the astral page is on top (`NavigatorBox gestureRecognizer:shouldReceiveTouch:`, encoding checked).

On the SE the page opened from the astral row with Discord's header, back arrow, section and three rows. Back returned to the overview with one astral category; an edge swipe left the page in place; the guest's close button closed Settings; reopening Settings recreated exactly one astral category, and the page opened again. The host test covers page open, close, close on a guest navigator change, and a second navigator under ASan and UBSan.

The workspace moved from `/tmp/project-astral`, which the system's periodic `/tmp` cleanup damaged overnight, to a durable clone.

## Loader 0.5.0

The Phoenix settings path moved from the probe into the loader:

- `PhoenixWire.c` holds the rewriter, templates, navigator tracking and a page builder driven by a description: sections of link, switch or info rows. Switch rows follow Discord's switch shape: the row has role `switch` and a checked flag (property 3) and subscribes to `accessibilityTap`; the track is a 56 by 28 view with radius 16, a `press` subscription and colour `0xFF5865F2` when on or `0x1F000000` when off; the thumb is a white 20-point circle whose offset (property 535) is 28 when on and 0 when off. A refresh rebuilds the page's sections in place: new section nodes, an op 3 on the list, and op 4 for the old ones.
- `PhoenixSettings.mm` installs the route after locating `opsArrived` and the host-call entry and checking `GuestTapRecognizer` and the `NavigatorBox` method encoding, and returns the reason when one fails. `PhoenixHostCall.c` finds the guest's `{context, call, drop}` record in the small-allocation heap (read with `vm_read_overwrite`) and swaps `call` atomically; `status.json` reports the route as `host-call`, `host-call-pending` or `host-call-missing`. It delivers open, refresh and close batches under the mutex, routes taps by identifier (`settings.row.astral`, `astral.back`, `astral.row.<key>`, `astral.switch.<key>`) and blocks the navigator's swipe while the page is open.
- `Bootstrap.mm` builds the page from loader state: a switch per installed plugin, the text style test (an info row while a plugin is active), and diagnostics. Actions reuse the existing plugin and test logic. If the Phoenix path refuses, the reason is written to `status.json` and the earlier UIKit entry is installed as the fallback so recovery stays reachable.

On the SE, 0.5.0 reported `settings: phoenix` with no refusal. The overview showed only the native astral category; the page showed Phoenix switches; switching Cyan Text on started the plugin (`plugin-active`), turned the switch on and made the style test unavailable, and the plugin styled Phoenix text including the page. Switching it off stopped the plugin, Back returned to the overview with one category, and the page reopened. `astral/loader/tests/phoenix_wire_test.c` covers the builder under ASan and UBSan.

## One route for jailed and jailbroken installs

The Rust guest reaches the host through a callback record Swift builds when Phoenix starts: `{context, call, drop}` (`0x7cf00` and `0x7cfc4` on 356.0). `call(context, frame, length, result)` hands every host call to the dispatcher (`0x3b3c0`), which treats a frame starting with a zero `uint32` as an ops batch and passes the rest to `UIManager.opsArrived`; for those frames it never writes `result`. Discord is a plain arm64 app, so the stored pointer is unsigned and can be replaced with an aligned store. Replacing it needs no writable code, no ElleKit and no JIT, so the loader links only system libraries and the package no longer depends on ElleKit. Batches sent before the swap (the first 20 to 100 ms of the guest's life) pass through unchanged.
