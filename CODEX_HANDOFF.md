# Handoff: astral Android parity and what follows

Branch `feat/android-parity` (pushed, tip `1cfedd3`). Everything up to `55a6290` is already on `main`. Before this branch the convention was to push astral straight to `main`. Merge the branch when you're happy with it.

Repo: `~/Developer/astral` (private `astral-app/astral`). Write astral in lowercase everywhere. Add no code comments and no AI attribution. Tools are Go only, manifests are TOML and plugins are Lua only. One code path serves jailed and jailbroken installs. Never cut a release.

## The user's request, in order

1. ~~Typing Avatars and Reaction Avatars~~: done on `main` (`00aa147`, `c501c8f`), verified on the vphone.
2. **astral on Android at 1:1 parity with iOS.** Every plugin should work. Port Unbound's loader-side work: the sideloading and environment fixes, and the display-cutout easter egg (the logo in the Dynamic Island, notch or punch hole: `packages/loader/core/src/LoaderDynamicIsland.cpp`, `ios/sources/Utilities+DynamicIsland.mm` in `~/Developer/unbound`). astral iOS lacks those loader features too, so add them on both platforms.
3. **Expand the plugin API** until essentially every Unbound plugin could be ported (more ports come after parity). Design each host feature as a general Lua API that works on both platforms.
4. **Port the user's Safari extension** with its existing features. Port it; don't reuse it as-is.
5. **Passkey support** in astral.

## How Android works now

- Discord's Android alpha (356.7, `com.discord`) runs **Phoenix**: the Rust guest is linked into `liblibdiscord_wasm_host.so` and hosted by Kotlin in `com.discord.nativeapp`.
  - The client is chosen by the shared preference `discord_fat_client.xml` → `launch_client=phoenix`, which is set on the AVD.
  - The APK splits are in `~/Developer/astral-work/discord-android-alpha/`. They came from Google Play through Unbound's tool (`cd ~/Developer/unbound/packages/loader && go run ./tools discord-apk --channel alpha --output-dir … --split base --split config.arm64_v8a --split config.xxhdpi --split config.en`).
  - jadx output is in `…/jadx/`; run jadx with `JAVA_HOME=~/Library/Caches/unbound/jdk/jdk-21.0.1+12/Contents/Home`, since Homebrew JDKs SIGBUS on macOS 27.
- **ViewWire** frames are byte-identical to iOS:
  - `GuestHost.invoke(frame)` with four leading zero bytes is a batch, handed to `GuestHost.applyOps(byte[], int)`. astral hooks `applyOps` (`packages/android/module/.../PhoenixBridge.kt`), and its own updates go back through the captured instance.
  - Events from host to guest go through `GuestHost.d(byte[], Function0)` as `[1, 1, int32 node, int16 eventId, payload]`. **They are not intercepted yet**, so taps on astral's settings rows do nothing on Android. Catch node ids in astral's ranges (0x7a…–0x7c…) there and route them to the page system.
- The property-name table is a static `String[]` in an obfuscated abstract class (`lh.c2` in 356.7) with exactly two static fields: `String[]` and `HashMap`. `PropertyTable.kt` finds it per APK and caches the class name.
- Module: `packages/android`, a libxposed API 101/102 module, one path for LSPosed (root) and LSPatch (sideload). CMake builds the whole core, the wire layer, Lua and the vendored SQLite 3.54.1 (`vendor/sqlite`) into `libastral.so`. Gradle stages astral's resources (`astral stage`) as assets, and `Resources.kt` extracts them once per APK.
  - Build: `cd packages/android && JAVA_HOME=<temurin 21.0.1> ./gradlew :module:assembleDebug`.
  - Install it, enable it in Vector and scope it to Discord: `adb shell su -c "sh /data/adb/modules/zygisk_vector/cli modules enable app.astral.android"`, then `… scope set app.astral.android com.discord/0`.
  - The Unbound module is disabled on the AVD.
- **Shared core** (moved this session; iOS keeps thin adapters):
  - `packages/core/wire/` (the rewriter)
  - `AstralLuaHost` (plugin host, `LuaHostPlatform` callbacks)
  - `AstralDiscordCache` (Phoenix's `files/store/<account>/cache.sqlite`, the same layout on Android)
  - `AstralDevServer` (core routes plus a platform `routes` hook)
  - `AstralReactors` (platform fetcher)
  - `AstralSession`
  - `AstralRequestSink` (a loopback 204 server for blocked requests)
- `astral dev` reaches Android with `DEVICE=android` (adb forward, pairing read with `su`). New commands: `astral dev enable|disable <plugin id>`, also on iOS. Developer mode is on in the AVD's `files/astral/settings.json`.

## Verified on the AVD

- astral's settings category shows in Discord's Settings.
- All 7 bundled plugins load. Role Color Everywhere, Mention Avatars, Typing Avatars, More User Tags and Reaction Avatars run, as `astral dev enable` reports.
- `astral dev status/eval/enable` work.

Not yet confirmed on Android:

- The visible output of each plugin (pick a channel with mentions and role colours).
- Telemetry blocking: the Cronet hooks install, but Phoenix's API traffic **does not go through Cronet**. The guest makes its own HTTP calls, so blocking has to happen elsewhere. Look for the guest's network path or a host module.

## Next step I was on: the session token on Android

- No Kotlin code sees `Authorization`. The token lives in `shared_prefs/wasm_secure_storage.xml` under `<account>:token` (skip `multi:`), stored as `2:` + base64(AES-GCM: 12-byte IV ‖ ciphertext ‖ tag).
- The data key is `shared_prefs/wasm_secure_key.xml` → `data_key`: base64 of the same GCM layout, encrypted with the AndroidKeyStore key alias `wasm_storage`. See `com.discord.nativeapp.modules.zm.c()/d()`, `xe.w()` and `bk.p()` in jadx.
- astral runs in Discord's process, so it can open AndroidKeyStore `wasm_storage`, unwrap `data_key`, decrypt the token, and call `Native.setSessionToken`. Then Reaction Avatars on Android should fill in. They are wired through `platform.reactors` → `AstralReactors` → `PhoenixBridge.fetch`/`Http.kt`.
- Never print or log the token.

## Remaining Android parity checklist

Compare with iOS `packages/ios/sources`:

- [ ] Event interception and the settings pages (astral, Plugins, Design, Marketplace, Updates). `SettingsPages.mm` builds the models in ObjC; move the model building into the core first.
- [ ] The `res://astral_galaxy` icon and astral's own images on Android (iOS uses `ImageAssets.m` and the `imageNamed:` hook). Find Phoenix Android's resource-image path.
- [ ] Themes, palettes and Nitro gradients (`Themes.mm` → core), and icon packs (resource hook).
- [ ] Emoji packs: the rewriter part is shared already; the resolver lives in `EmojiPacks.mm` and the composer overlay in `EmojiComposer.mm` (UIKit only; Android needs its own EditText or Phoenix text-input path).
- [ ] Network filter, Staff DevTools and gateway injection: iOS uses `NetworkFilter.mm` with NSURLProtocol and the WebSocket receive hook.
- [ ] Marketplace and catalog, signature checks (`PluginSignature.mm` uses CommonCrypto SHA-256; port it to the core), backups, crash reports, soft restart, author avatars.
- [ ] Sideload and environment fixes, plus the cutout easter egg, ported from Unbound (`packages/loader/android/module` and `ios/sources/Sideloading*`, `EnvironmentReport*`) on both platforms.
- [ ] A Go command to build and deploy the Android module, mirroring `astral vphone deploy`, plus CI for `packages/android`.
- [ ] Re-verify iOS on the vphone. The Lua host, Discord cache, dev server and reactors moved into the core, and iOS builds and its tests pass, but nothing has run on the device since.

## Devices and rules

- Never run the vphone and the AVD together.
  - AVD: `~/Developer/unbound/skills/ubd-avd/scripts/up.sh emu`; stop it with `tmux kill-session -t avd-emu`.
  - vphone: `vphone-cli vm stop vphone`, boot with `astral vphone boot`, deploy with `astral vphone deploy`.
- Never touch the user's personal iPhone 15 Pro Max. Unbound stays on the SE; astral runs on the vphone and AVD.
- Change no Discord account state.
- Test messages may go only to botcave `#development` (guild 709061970078335027, channel 709062739581992971). Ask before posting anywhere else.
- The AVD's composer holds a draft "hello there". Don't send it.
- Signing keys are in iCloud (`astral-plugin-signing/`). `astral sign` re-signs plugins and the catalog. Never print or regenerate keys.
