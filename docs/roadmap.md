# astral roadmap

astral is complete when every supported Phoenix behaviour works on iOS and Android, installs keep Discord's own app extensions, and plugin authors have a documented cross-platform API. Unbound is the feature reference.

## Where it stands

| Area | State |
| --- | --- |
| Shared core | Rust: ViewWire rewriter, Lua host, settings navigator, cache, plugins, themes, packs, catalog, `/debug` and diagnostics |
| iOS | Theos package and sideload IPA; settings, plugins and reports run on the Rust app layer; Phoenix access gate opened for non-testers |
| Android | Xposed module and LSPatch APK; same Rust app layer; Phoenix access gate opened at build time |
| Plugins | 44 signed Lua ports of Unbound plugins ([parity](unbound-parity.md)) |
| Plugin API | Generic primitives: `ui.patch`, `ui.insert`, `ui.style_text`, sheets, message actions, commands, `discord.request`, `net.intercept`, gateway events, files, media, timers, storage |
| Tooling | `astral` CLI in Rust; vphone and AVD deploy, dev server, signing, compatibility scans |

## Open work

1. **Device acceptance.** Run every install mode on the vphone and the AVD, one at a time. Record the Phoenix version, plugin results, settings navigation, packs, backups, restarts and logs.
2. **Phoenix unlock on a non-tester account.** The access-gate patch is verified to apply without crashing; opening Phoenix on an account outside the test group is still unproven.
3. **Generic APIs.** Long-press on inserted nodes and Discord's markup view, once a plugin needs them.
4. **API gaps.** Richer inserts (video, Lottie, markup, long-press), `files.info`, Discord settings overrides and a server long-press hook.
5. **Port parity.** Close the gaps in [unbound-parity.md](unbound-parity.md#known-gaps) and mirror Unbound plugin fixes as they land.
6. **Compatibility automation.** A per-build function map in CI, the guest kind in reports and a seam diff in compatibility scans.
7. **Release.** Document installation and recovery for each mode, then publish once the device matrix and CI pass.

## Rules

- A capability is done when its API, both platform adapters, tests and device evidence agree.
- Run the vphone and the AVD one at a time.
- Keep Discord account state unchanged during validation.
- Keep decrypted Discord apps and account data out of CI and the repository.
- astral stays lowercase in visible text, docs and commits.
