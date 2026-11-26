<div align="center">

<!-- TODO: add the logo here once it exists: <img src="./packages/assets/logo.png" width="120" alt="astral logo" /> -->

<h1>astral</h1>

<b>A native client mod for Discord on iOS and Android.</b>

Lua plugins, themes, icon packs and emoji packs, drawn by Discord's own components.

<br />

[![Stars](https://img.shields.io/github/stars/astral-app/astral?style=for-the-badge&labelColor=0c0c0c&color=7c6cf2)](https://github.com/astral-app/astral/stargazers)
[![Plugins](https://img.shields.io/badge/plugins-Lua%205.4-7c6cf2?style=for-the-badge&labelColor=0c0c0c)](packages/addons/plugins/README.md)
[![Platforms](https://img.shields.io/badge/platforms-iOS%20%7C%20Android-7c6cf2?style=for-the-badge&labelColor=0c0c0c)](#installation)

**[Plugins](packages/addons/plugins/README.md)** · [Plugin API](docs/plugins.md) · [Roadmap](docs/roadmap.md)

</div>

<!-- TODO: add a hero screenshot or demo GIF of astral running here. -->

## Contents

- **For Users:** [What Is astral?](#what-is-astral) · [Features](#features) · [Installation](#installation)
- **For Developers:** [Writing Plugins](#writing-plugins) · [Developing astral](#developing-astral) · [How It Works](#how-it-works) · [Layout](#layout)
- **Project:** [Contributing](#contributing) · [Acknowledgements](#acknowledgements) · [Star History](#star-history)

## For Users

### What Is astral?

astral mods Discord's Phoenix engine, the app where a Rust guest describes the UI and a native host draws it.

The guest sends the UI to the host in ViewWire batches. astral rewrites those batches, so everything it adds is a real Discord component: an astral category in Settings, decorations in message headers, themes.

It patches no code. Jailed and jailbroken installs run the same path, and no single Discord build is required.

Plugins are sandboxed Lua 5.4 files. They run no native code, so a plugin cannot crash Discord.

### Features

| Feature              | Description                                                                     |
| -------------------- | ------------------------------------------------------------------------------- |
| Plugins              | [40+ bundled Lua plugins](packages/addons/plugins/README.md), signed and verified on install. |
| Native settings      | An astral category in Discord's Settings, built from Discord's own rows.        |
| Themes               | Bundled themes, plus more in the marketplace catalog.                           |
| Icon and emoji packs | Liquid Glass icons and Fluent emoji out of the box.                             |
| Marketplace          | A signed default catalog of plugins and themes.                                 |
| Cross-platform       | iOS and Android share one core: the rewriter, the Lua host and the settings pages. |

### Installation

There is no release yet. The [Release artifacts](https://github.com/astral-app/astral/actions/workflows/artifacts.yml) workflow builds every package weekly, with checksums and provenance.

| Platform              | Method                                                                                         |
| --------------------- | ---------------------------------------------------------------------------------------------- |
| iOS, sideloaded       | Sideload IPA. It installs next to stock Discord: astral takes `astral://` and leaves `discord://`. |
| iOS, jailbroken       | The `.deb` package.                                                                            |
| Android, with Xposed  | The astral module, scoped to Discord in Vector.                                                |
| Android, without root | An LSPatch-patched Discord APK.                                                                |

The sideload IPA ships an Open in astral Safari extension for Discord links.

## For Developers

### Writing Plugins

1. Scaffold a plugin with a test and translations:

    ```bash
    astral create <id>
    ```

2. Write it against the `astral` API in [docs/plugins.md](docs/plugins.md). Manifests, themes and the catalog are TOML.
3. Turn on Developer mode in astral, then push on every save:

    ```bash
    DEVICE=<udid> astral dev watch <plugin folder>
    ```

4. Run `astral readme` to update the [plugin list](packages/addons/plugins/README.md). The icon PNG goes in `packages/assets/plugin-icons/`; `astral test` fails when the list is stale.

Working outside this repository? `astral sdk install` adds the API types to lua-language-server.

`astral dev` forwards the device's development API (`127.0.0.1:9324`) over USB:

| Command                         | What it does                    |
| ------------------------------- | ------------------------------- |
| `status`                        | Device and runtime status.      |
| `push <plugin folder>`          | Install and hot reload.         |
| `watch <plugin folder>`         | Push on every save.             |
| `eval <plugin id> <lua>`        | Run Lua inside a plugin.        |
| `logs [-f]`                     | Read or follow the logs.        |
| `theme import`, `backup export\|import` | Move themes and backups. |

### Developing astral

Requirements: Rust. For iOS, Theos and GNU make (`astral prepare-theos`); Xcode on macOS for IPAs. For Android, JDK 21 and the Android SDK.

```bash
git submodule update --init
git config core.hooksPath .githooks
cargo astral lint
cargo astral test
make -C packages/ios package FINALPACKAGE=1
```

`cargo astral <command>` builds and runs the CLI, leaving it at `packages/tools/target/release/astral`.

| Command                                         | What it does                                                                                                   |
| ----------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `format`, `lint`                                | Format and check Lua (StyLua, luacheck, lua-language-server), TOML (Taplo), C and Objective-C (clang-format) and Rust (rustfmt, clippy). |
| `test [--phoenix <PhoenixFatClient>]`           | The ViewWire host test under ASan and UBSan, the prelude and plugin tests, and every signature. `--phoenix` adds the locator test. |
| `fuzz`, `bench`                                 | libFuzzer over the rewriter, and its cost per batch. CI runs both.                                             |
| `sign`                                          | Sign the plugins and the catalog with the key in iCloud.                                                       |
| `wire-dump`                                     | Pull and decode the batches a test device recorded while `Documents/astral/capture-wire` exists. Text is kept as length and hash. |
| `compat --ipa <Discord.ipa>`                    | Check an iOS build for the hook points and view properties astral needs.                                       |
| `android-compat --apk <apk> [--apk <split>...]` | Scan an Android APK and its splits for Phoenix host classes, hook signatures, ViewWire properties and events. `--channel <stable\|beta\|alpha>` downloads the latest arm64 APKs and checks Discord signed them. |
| `deploy`                                        | Build and install the iOS package on a local test device.                                                      |
| `build-ipa --ipa <decrypted Discord.ipa>`       | Build a sideload IPA. It keeps Discord's own extensions and checks the signed app still wires each one.        |
| `sideload --ipa <IPA> --bundle-id <id> --device <udid>` | Build the IPA if needed, sign it with development profiles, validate it and install over USB.          |
| `android build`, `android download --channel <c>` | Build the Android module, or fetch a channel's Discord APK and splits.                                       |
| `android deploy --device <serial> --mode module\|sideload` | Install the module and scope it in Vector, or install a patched app. Both upgrade in place.           |

A scheduled workflow runs the compat checks on every new Discord build: iOS stable and beta, Android stable, beta and alpha. Alpha breakages are warnings, not issues.

#### The vphone

`astral vphone` drives the iOS VM through its guest API:

1. `astral vphone boot` restarts the VM with the API on. The token lives in `~/.config/astral/vphone-token`.
2. `astral vphone discord --ipa <Discord.ipa>` installs stock Discord.
3. `astral vphone deploy` builds astral and deploys it through the VM's injection setup.

`status`, `info`, `install`, `launch`, `push`, `pull` and `shell` cover the rest.

> [!WARNING]
> Keep test data inside the VM, never change live account state while validating, and never run the vphone and the Android emulator together.

### How It Works

On iOS, astral finds its hooks at runtime instead of patching code:

1. It finds Phoenix's `opsArrived` as the one function that references the `ops batch dropped` diagnostic (via `LC_FUNCTION_STARTS`).
2. It walks up to the host-call entry the Rust guest calls for every frame: the unique caller of the unique caller of the dispatcher that calls `opsArrived`.
3. It caches both per Phoenix UUID in `Documents/astral/hooks.json`.
4. The guest keeps that entry in a `{context, call, drop}` record on the heap. astral swaps `call` for its own function.
5. That function rewrites ops frames (a zero `uint32`, then the batch) and sends astral's own batches through the original.

At startup, astral alerts if an app extension it expects is missing. On Phoenix 356.0 (build 124376), Phoenix's native passkey module is present, so astral lets it finish passkey login itself; `browserLogin` in the runtime report shows these checks.

### Layout

| Path                                                        | Contents                                                                                     |
| ----------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| [`packages/core`](packages/core)                            | Shared ViewWire rewriter, settings pages, Lua host, Discord cache, appearance services and Lua prelude. |
| [`packages/ios`](packages/ios)                              | iOS integration and platform services.                                                       |
| [`packages/android`](packages/android)                      | Android module, JNI bridge and platform services.                                            |
| [`packages/addons/plugins`](packages/addons/plugins)        | Bundled Lua plugins and their tests.                                                         |
| [`packages/addons/themes`](packages/addons/themes), [`packages/catalog`](packages/catalog) | Bundled themes, Discord's raw palette and the default marketplace catalog. |
| [`packages/addons/icon-packs`](packages/addons/icon-packs), [`packages/addons/emoji-packs`](packages/addons/emoji-packs) | Icon packs (PNGs named after Phoenix's icons) and emoji packs (image URL templates and coverage lists). |
| [`packages/sdk`](packages/sdk)                              | The plugin API as a lua-language-server addon (`config.json`, `library/`).                   |
| [`packages/assets`](packages/assets)                        | Icon SVGs served as `res://astral_<name>`.                                                   |
| [`packages/tools`](packages/tools)                          | The `astral` CLI, in Rust.                                                                   |
| [`keys`](keys)                                              | The public key plugins and the catalog are verified against.                                 |
| [`vendor`](vendor)                                          | Lua 5.4 (submodule). SQLite comes from the `libsqlite3-sys` crate.                           |
| [`docs`](docs)                                              | How the Phoenix integration and plugins work.                                                |

## Project

### Contributing

1. Run `git config core.hooksPath .githooks`. The pre-commit hook runs lint and tests.
2. Run `cargo astral format`, `lint` and `test` before committing.
3. Commit as `type(scope): subject`. Scopes: `ios`, `core`, `plugins`, `sdk`, `themes`, `catalog`, `tools`, `docs`.
4. Write astral in lowercase, everywhere.
5. Keep account data, screenshots, IPAs and decrypted binaries out of the repository.

New capabilities go into the `astral` Lua API, never into native plugin code. Nothing may need a jailbreak.

### Acknowledgements

astral follows [Unbound](https://github.com/marioparaschiv/unbound) closely: its catalog, plugin icons, author cards and locales. [docs/unbound-parity.md](docs/unbound-parity.md) tracks the gap.

<div align="center">

<a href="https://github.com/astral-app/astral/graphs/contributors">
	<img src="https://contrib.rocks/image?repo=astral-app/astral" alt="Contributors" />
</a>

</div>

### Star History

<div align="center">

<a href="https://star-history.com/#astral-app/astral&Date">
	<img src="https://api.star-history.com/svg?repos=astral-app/astral&type=Date" alt="Star history chart" width="600" />
</a>

</div>
