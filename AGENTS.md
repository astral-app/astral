# astral

- Run `tools/test.sh` before committing; it builds the ViewWire host test under ASan and UBSan and runs every plugin's Lua tests.
- Commit as `type(scope): subject`, with scope `loader`, `core`, `plugins`, `themes`, `tools` or `docs`.
- Plugins are Lua only; new capabilities go into the `astral` API, never into native plugin code.
- Keep Discord account data, screenshots, IPAs and decrypted binaries out of the repository.
