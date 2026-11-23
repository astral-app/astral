# astral completion roadmap

astral is a native Discord Phoenix client modification with a shared Lua runtime. Completion means supported Phoenix behavior works across both platforms, packaged installs retain Discord's own app extensions, and add-on authors have a documented cross-platform API. Unbound is the feature reference; its React Native, Metro and Flux APIs are not assumed to map directly to Phoenix.

## Delivery status

The branch adds post-sign extension checks, installed-extension diagnostics and Phoenix passkey runtime reporting. The iOS vphone run reached `ready` on Phoenix 356.0 build 124376, confirmed both native passkey delegate methods, and loaded the bundled plugin set. Live acceptance remains open for Safari fallback activation, a signed sideload IPA and Android runtime behavior; no authorized Phoenix IPA or APK/splits were available.

| # | Workstream | Branch result | Remaining acceptance |
| --- | --- | --- | --- |
| 1 | Device acceptance | iOS vphone reached `ready` on Phoenix 356.0 and reported all bundled plugins without runtime errors; iOS package and Android module builds pass | Run jailed iOS and all Android install modes against authorized Phoenix artifacts; exercise plugin UI and settings flows |
| 2 | Android network interception | Cronet, standard OkHttp, Phoenix's shaded OkHttp and WebSocket hooks are implemented; Android module build passes | Verify live telemetry sink responses, ordinary API requests and Staff DevTools on an Android Phoenix build |
| 3 | iOS login and extensions | `astral://`, Safari extension packaging, source-extension inventory, post-sign presence checks and group-remap checks are implemented; vphone confirms Phoenix's native passkey delegates | Verify Safari fallback, signing, upgrade and Share/BroadcastUpload on an authorized sideload IPA; do not change a logged-in account to force a login test |
| 4 | Environment reporting | Schema 2 reports include platform/install facts and sanitize details on both platforms; iOS runtime status was read from the vphone | Compare real sideload, package and Android module reports on devices |
| 5 | Lua API | Permission-gated native alerts, clipboard writes, haptics, multiline text settings, file selection, lifecycle events and metadata-only message-create events are implemented on iOS and Android | Exercise each adapter in Phoenix; commands, arbitrary Discord event payloads, notifications, background work, fonts and general asset registration remain unsupported |
| 6 | Recovery and privacy | Plugin failures stay isolated; reports scrub new and legacy details; Android records available process crash/ANR traces | Exercise disable, reload, soft restart and launch recovery across install modes |
| 7 | Compatibility automation | Android DEX scanner, Safari extension CI build and synthetic IPA preservation plus post-sign validation tests are present | Scan authorized Android APKs/splits; complete scheduled iOS scan after an upstream artifact is available |
| 8 | Add-on corpus | Nine signed Lua plugins; Text Replace has editable multiline rules, text-file import, clipboard export and Lua tests | Verify buffered message traffic on Phoenix and the native rule editor on both platforms |
| 9 | Documentation | README and plugin docs state supported APIs, permissions, install behavior, `astral://` and known gates | Review release notes after device acceptance closes |

## Current baseline

| Area | Current state | Remaining proof |
| --- | --- | --- |
| Shared runtime | ViewWire rewriting, Lua host, settings pages, cache, plugins, themes and package services are in shared core | Run acceptance on each supported install path |
| iOS | Theos package and sideload IPA tooling; Safari extension; `astral://` scheme; source app extensions recorded, preserved and checked after signing; Phoenix native passkey delegate detected on vphone | Verify signed IPA contents and extension behavior, then test browser fallback only on a build that lacks Phoenix's native passkey module |
| Android | Phoenix module and patched APK paths; settings, plugins, themes, packs, backups, catalog, network hooks and static compatibility scanner | Scan an authorized Phoenix APK and validate behavior on a live build |
| Add-ons | Ten bundled Lua plugins, one bundled theme, three catalog themes, one icon pack and one emoji pack; signed catalog verification is in the test suite | Verify Clear URLs, Text Replace and Mention Haptics on live Phoenix traffic, then expand after additional native capabilities are supported |
| Compatibility | iOS stable and beta scans; Android DEX scanner and module/core builds in CI | Run Android scan against a current authorized APK |
| Diagnostics | Schema 2 reports; iOS MetricKit; Android process crash/ANR exit details when available | Compare live reports and recovery behavior on both platforms |

## Acceptance matrix

| Capability | iOS sideload | iOS jailbreak | Android patched APK | Android module |
| --- | --- | --- | --- | --- |
| Phoenix injection and shared Lua host | Implemented; device acceptance pending | Implemented; device acceptance pending | Implemented; device acceptance pending | Implemented; device acceptance pending |
| Settings, plugin lifecycle and UI decorations | Implemented; representative plugin checks pending | Implemented; representative plugin checks pending | Implemented; representative plugin checks pending | Implemented; representative plugin checks pending |
| Themes, images, emoji and icon packs | Implemented; behavior check pending | Implemented; behavior check pending | Implemented; behavior check pending | Implemented; behavior check pending |
| Telemetry filtering and Staff DevTools | Implemented; live traffic check pending | Implemented; live traffic check pending | Implemented; Cronet and OkHttp checks pending | Implemented; Cronet and OkHttp checks pending |
| Native alerts, file selection and lifecycle events | API and adapter implemented; device check pending | API and adapter implemented; device check pending | API and adapter implemented; device check pending | API and adapter implemented; device check pending |
| Backups, reports and restart recovery | Implemented; compare report and recovery behavior | Implemented; compare report and recovery behavior | Implemented; compare report and recovery behavior | Implemented; compare report and recovery behavior |
| Safari passkey handoff | Phoenix 356.0 has native module and success/failure delegate selectors; direct native path detected in vphone status; Safari fallback activation not verified | Native Phoenix authorization is detected at runtime; Safari fallback is not part of the stock package | Not applicable | Not applicable |
| Discord Share and BroadcastUpload extensions | Source inventory, group adapter and signed-app validation have synthetic coverage; signed install pending | Stock app extensions remain stock; startup diagnostics only activate for astral installs | Not applicable | Not applicable |

## Acceptance work

1. **Finish device acceptance.** Test iOS and Android sequentially on the vphone and AVD. Record Phoenix version, install mode, plugin results, settings navigation, packs, backups, restart and logs. Confirm Mention Avatars, Role Color Everywhere and Typing Avatars on Android.

2. **Verify Android network interception.** Exercise actual Phoenix Cronet and shaded OkHttp paths. Confirm telemetry requests increment the blocked counter and receive the local sink response, while ordinary Discord API traffic and `astral.net.request` still work. Verify Staff DevTools changes gateway and current-user responses only when enabled.

3. **Verify iOS browser login and IPA extensions.** Install an IPA built from a local Discord test artifact. Exercise Safari extension enabled, disabled, cancel, retry and expiry paths. Confirm Phoenix completes account activation after the server handoff reaches `astral://`. Verify Share, BroadcastUpload and every source app extension survives packaging, group remapping, signing and upgrade-in-place.

4. **Complete install-mode and environment review.** Compare Unbound's environment and sideload handling with astral's adapters. Validate schema 2 reports and install-mode facts separately for jailed/sideloaded and injected installs. Keep the Phoenix core path identical.

5. **Exercise the Lua API.** Test `astral.ui.alert`, `astral.ui.copy`, `astral.ui.haptic`, `astral.settings.text`, `astral.files.pick`, lifecycle subscriptions and metadata-only `message.create` events on both platforms, including permission denial, editor cancellation, picker cancellation, oversized files and plugin disable. Add commands, notifications, fonts, assets or arbitrary Discord event payloads only when Phoenix offers a supported native seam.

6. **Validate recovery and report privacy.** Test plugin failure isolation, disable, reload, soft restart and launch recovery. Confirm reports contain no account tokens, message contents or private paths, including reports created by older astral versions.

7. **Run compatibility automation.** Build the Safari extension and synthetic IPA tests in CI. Run `astral android-compat` with an authorized base APK and each split APK; retain only the JSON/Markdown result. Keep decrypted Discord apps and account data out of CI and the repository.

8. **Grow the add-on corpus.** The shared pre-send transform, Clear URLs, Text Replace and Mention Haptics ports are implemented. Text Replace uses editable literal or Lua pattern rules with optional context, imports `.txt` rules and exports through the clipboard. Add the row, contextual-action and native UI seams for the remaining user-selected shortlist: Highlight Replies, Client Side Block (Enhanced Blocking), Chat Bubbles, Translate, Expression Tools and Live Markdown. Add Lua tests, sign completed packages and update the catalog signature.

9. **Close release documentation.** Document installation and recovery for each supported mode, the `astral://` scheme, Safari passkeys, extension preservation, plugin permissions, API reference and compatibility results. Publish no release until the device matrix and CI gates pass.

## Rules for closing work

- A capability is complete when its API, both platform adapters, tests and device evidence agree.
- Run the iOS virtual device and Android emulator one at a time.
- Keep Discord account state unchanged during validation and do not send the AVD's existing draft.
- Use synthetic IPA fixtures in CI. Keep private Discord artifacts out of the repository.
- astral remains lowercase in visible text, docs and commits. Plugins remain Lua; platform capabilities belong in astral's API.
