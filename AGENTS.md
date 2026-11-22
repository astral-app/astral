# astral

- Run `tools/format.sh`, `tools/lint.sh` and `tools/test.sh` before committing (the pre-commit hook in `.githooks` runs lint and tests); the tests build the ViewWire host test under ASan and UBSan and run every plugin's Lua tests.
- astral is always written lowercase in visible text, docs and commits.
- Commit as `type(scope): subject`, with scope `loader`, `core`, `plugins`, `themes`, `tools` or `docs`.
- Plugins are Lua only; new capabilities go into the `astral` API, never into native plugin code.
- Keep Discord account data, screenshots, IPAs and decrypted binaries out of the repository.
