# astral

A native client mod for Discord's Phoenix engine on iOS, with Lua plugins.

astral rewrites the ViewWire batches Discord's Rust guest sends to its Swift host, so everything it adds is drawn by Discord's own components: an astral category in Settings with native pages, decorations in message headers, and themes. Plugins are sandboxed Lua files.

## Layout

| Path | Contents |
| --- | --- |
| `packages/loader-ios` | The tweak: ViewWire rewriter, settings pages, Lua host, Discord cache reader |
| `packages/core` | Shared C++ core and the Lua prelude every plugin runs on |
| `packages/plugins` | Bundled Lua plugins and their tests |
| `packages/sdk` | lua-language-server definitions for the plugin API |
| `packages/themes`, `packages/catalog` | Bundled themes, Discord's raw palette and the default marketplace catalog |
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
make -C packages/loader-ios package FINALPACKAGE=1
```

`astral format` formats Lua (StyLua), TOML (Taplo), C and Objective-C (clang-format) and Go (gofmt); `astral lint` checks the same, lints Lua with luacheck and lua-language-server and vets Go. `astral test` runs the ViewWire host test under ASan and UBSan, the prelude and plugin tests, and verifies every signature with Go and with Monocypher (`--phoenix <PhoenixFatClient>` adds the locator test). `astral sign` signs the plugins and the catalog with the key in iCloud. Plugin manifests, themes and the catalog are TOML; internal state the loader writes stays JSON.

With Developer mode on, the loader serves a development API on the device's `127.0.0.1:9324`. `astral dev` forwards it over USB (`DEVICE=<udid>`) and offers `status`, `push <plugin folder>` (install and hot reload), `watch <plugin folder>` (push on every save), `eval <plugin id> <lua>`, `logs [-f]`, `theme import` and `backup export|import`. `astral wire-dump` pulls the batches a test device recorded while `Documents/astral/capture-wire` exists and decodes them, with text kept as length and hash.

Theos and GNU make are required (`astral prepare-theos`). `astral deploy` builds and installs on a jailbroken test device (`DEVICE=<udid>`), and `astral build-ipa --ipa <decrypted Discord.ipa>` produces a sideloadable IPA that needs no jailbreak and bundles no hooking library.

astral is not tied to one Discord build, and it patches no code, so jailed and jailbroken installs run the same path. It finds Phoenix's `opsArrived` by the one function that references its `ops batch dropped` diagnostic (via `LC_FUNCTION_STARTS`), then the host-call entry the Rust guest calls for every frame (the unique caller of the unique caller of the dispatcher that calls `opsArrived`), and caches both per Phoenix UUID in `Documents/astral/hooks.json`. The guest keeps that entry in a `{context, call, drop}` record on the heap; astral finds the record and swaps `call` for its own function, which rewrites ops frames (a zero `uint32` followed by the batch) and sends astral's own batches through the original. `astral test --phoenix <PhoenixFatClient>` runs the locator test.
