# astral

A native client mod for Discord's Phoenix engine on iOS, with Lua plugins.

astral rewrites the ViewWire batches Discord's Rust guest sends to its Swift host, so everything it adds is drawn by Discord's own components: an astral category in Settings with native pages, decorations in message headers, and themes. Plugins are sandboxed Lua files.

## Layout

| Path | Contents |
| --- | --- |
| `packages/loader-ios` | The tweak: ViewWire rewriter, settings pages, Lua host, Discord cache reader |
| `packages/core` | Shared C++ core: settings store, files, JSON, logging, launch guard |
| `plugins/` | Bundled Lua plugins and their tests |
| `themes/`, `catalog/` | Bundled themes and the default marketplace catalog |
| `assets/` | Icon sources and the rendered PNGs served as `res://astral_<name>` |
| `vendor/` | Lua 5.4 (submodule) and nlohmann/json |
| `tools/` | Tests, packaging, sideload IPA builds and device scripts |
| `docs/` | How the Phoenix integration and plugins work |

## Build

```bash
git submodule update --init
git config core.hooksPath .githooks
tools/lint.sh
tools/test.sh
make -C packages/loader-ios package FINALPACKAGE=1
```

`tools/format.sh` formats Lua (StyLua), TOML (Taplo), C and Objective-C (clang-format) and shell (shfmt); `tools/lint.sh` checks the same and lints Lua with luacheck, TOML with Taplo and shell with shellcheck. Plugin manifests, themes and the catalog are TOML; internal state the loader writes stays JSON. With Developer mode on, the loader serves a development API on the device's `127.0.0.1:9324`. `tools/astral-dev` forwards it over USB (`DEVICE=<udid>`) and offers `status`, `push <plugin folder>` (install and hot reload), `watch <plugin folder>` (push on every save), `eval <plugin id> <lua>` (run Lua inside a running plugin) and `logs [-f]`. `tools/wire-dump.sh` pulls the batches a test device recorded while `Documents/astral/capture-wire` exists and decodes them, with text kept as length and hash.

Theos and GNU make are required. `tools/deploy.sh` builds and installs on a jailbroken test device (`DEVICE=<udid>`), and `tools/build-ipa.sh --ipa <decrypted Discord.ipa>` produces a sideloadable IPA.

astral is not tied to one Discord build, and it patches no code, so jailed and jailbroken installs run the same path. It finds Phoenix's `opsArrived` by the one function that references its `ops batch dropped` diagnostic (via `LC_FUNCTION_STARTS`), then the host-call entry the Rust guest calls for every frame (the unique caller of the unique caller of the dispatcher that calls `opsArrived`), and caches both per Phoenix UUID in `Documents/astral/hooks.json`. The guest keeps that entry in a `{context, call, drop}` record on the heap; astral finds the record and swaps `call` for its own function, which rewrites ops frames (a zero `uint32` followed by the batch) and sends astral's own batches through the original. Set `ASTRAL_PHOENIX_BINARY` to a PhoenixFatClient binary to run the locator test.
