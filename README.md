# astral

A native client mod for Discord's Phoenix engine on iOS and Android, with a shared Lua plugin runtime.

astral rewrites the ViewWire batches Discord's Rust guest sends to its native client host, so everything it adds is drawn by Discord's own components: an astral category in Settings with native pages, decorations in message headers, and themes. Plugins are sandboxed Lua files.

## Layout

| Path | Contents |
| --- | --- |
| `packages/ios` | iOS integration and platform services |
| `packages/android` | Android module, JNI bridge and platform services |
| `packages/core` | Shared ViewWire rewriter, settings pages, Lua host, Discord cache, appearance services and Lua prelude |
| `packages/addons/plugins` | Bundled Lua plugins and their tests |
| `packages/addons/icon-packs`, `packages/addons/emoji-packs` | Bundled icon packs (PNGs named after Phoenix's icons) and emoji packs (image URL templates and coverage lists) |
| `packages/sdk` | The plugin API as a lua-language-server addon (`config.json`, `library/`) |
| `packages/addons/themes`, `packages/catalog` | Bundled themes, Discord's raw palette and the default marketplace catalog |
| `packages/assets` | Icon SVGs served as `res://astral_<name>` |
| `packages/tools` | The `astral` command (Go): tests, lint, signing, packaging, sideload IPAs and device helpers |
| `keys/` | The public key plugins and the catalog are verified against |
| `vendor/` | Lua 5.4, Monocypher and toml++ (submodules) and nlohmann/json |
| `docs/` | How the Phoenix integration and plugins work |

## Build

```bash
git submodule update --init
git config core.hooksPath .githooks
go run ./packages/tools/cmd/astral lint
go run ./packages/tools/cmd/astral test
make -C packages/ios package FINALPACKAGE=1
```

`astral format` formats Lua (StyLua), TOML (Taplo), C and Objective-C (clang-format) and Go (gofmt); `astral lint` checks the same, lints Lua with luacheck and lua-language-server and vets Go. `astral test` runs the ViewWire host test under ASan and UBSan, the prelude and plugin tests, and verifies every signature with Go and with Monocypher (`--phoenix <PhoenixFatClient>` adds the locator test). `astral sign` signs the plugins and the catalog with the key in iCloud. Plugin manifests, themes and the catalog are TOML; internal state astral writes stays JSON.

With Developer mode on, astral serves a development API on the device's `127.0.0.1:9324`. `astral dev` forwards it over USB (`DEVICE=<udid>`) and offers `status`, `push <plugin folder>` (install and hot reload), `watch <plugin folder>` (push on every save), `eval <plugin id> <lua>`, `logs [-f]`, `theme import` and `backup export|import`. `astral wire-dump` pulls the batches a test device recorded while `Documents/astral/capture-wire` exists and decodes them, with text kept as length and hash.

`astral fuzz` runs libFuzzer over the rewriter (Homebrew LLVM locally) and `astral bench` reports its cost per batch; CI runs both. `astral compat --ipa <Discord.ipa>` checks a Discord iOS build for the hook points and view properties astral needs, and a scheduled workflow runs it on each new stable and beta build. `astral android-compat --apk <Discord.apk> [--apk <split.apk> ...]` scans an authorized Android base APK and its splits for Phoenix host classes, hook signatures, ViewWire properties and event identifiers. `astral create <id>` scaffolds a plugin with a test and translations, and `astral sdk install` installs the plugin API definitions as a lua-language-server addon for plugins outside this repository.

The vphone is driven through its guest API: `astral vphone boot` restarts the VM with the API on and a token kept in `~/.config/astral/vphone-token`. Install stock Discord with `astral vphone discord --ipa <Discord.ipa>`, then run `astral vphone deploy` to build and deploy astral through the VM's existing injection setup. `status`, `info`, `install`, `launch`, `push`, `pull` and `shell` cover the remaining device operations. Keep test-session data inside the VM, avoid changing any live account state during validation, and never run the vphone and the Android emulator together.

`astral android build` builds the Android module using JDK 21 and the Android SDK. To prepare a sideloaded installation, pass `--patcher <LSPatch.jar>` followed by the Discord base APK and its splits. `astral android deploy --device <adb serial> --mode module <astral-module.apk>` installs and scopes the module in Vector; `--mode sideload <patched APK>...` installs the patched app. Both upgrade in place and leave installation errors visible instead of uninstalling Discord.

Theos and GNU make are required (`astral prepare-theos`). `astral deploy` builds and installs the package for a local test device. On macOS with Xcode, `astral build-ipa --ipa <decrypted Discord.ipa>` produces a sideloadable IPA with the Open in astral Safari extension and an app-group adapter for Discord extensions. It keeps Discord's Share, BroadcastUpload and other source extensions. The packaged app registers `astral://`, leaving `discord://` for stock Discord. When Phoenix exposes its native `PasskeysModule` authorization delegates, astral lets Phoenix complete its built-in passkey flow directly. The Safari extension can create a one-time Discord auth handoff through `astral://` on builds that need the browser fallback; app-side activation of that fallback remains unverified. `astral sideload --ipa <IPA> --bundle-id <id> --device <udid>` builds the IPA if needed, signs the app and its extensions with compatible development profiles, and installs it over USB.

astral is not tied to one Discord build, and it patches no code, so jailed and jailbroken installs run the same path. It finds Phoenix's `opsArrived` by the one function that references its `ops batch dropped` diagnostic (via `LC_FUNCTION_STARTS`), then the host-call entry the Rust guest calls for every frame (the unique caller of the unique caller of the dispatcher that calls `opsArrived`), and caches both per Phoenix UUID in `Documents/astral/hooks.json`. The guest keeps that entry in a `{context, call, drop}` record on the heap; astral finds the record and swaps `call` for its own function, which rewrites ops frames (a zero `uint32` followed by the batch) and sends astral's own batches through the original. `astral test --phoenix <PhoenixFatClient>` runs the locator test.
