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
| `third_party/` | Lua 5.4 (submodule) and nlohmann/json |
| `tools/` | Tests, packaging, sideload IPA builds and device scripts |
| `docs/` | How the Phoenix integration and plugins work |

## Build

```bash
git submodule update --init
tools/test.sh
make -C packages/loader-ios package FINALPACKAGE=1
```

Theos and GNU make are required. `tools/deploy.sh` builds and installs on a jailbroken test device (`DEVICE=<udid>`), and `tools/build-ipa.sh --ipa <decrypted Discord.ipa>` produces a sideloadable IPA.

Discord 356.0 (build 124376) is the supported build; the Phoenix image is checked by UUID before anything is hooked.
