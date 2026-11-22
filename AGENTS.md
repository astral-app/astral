# astral

- Run `go run ./packages/tools/cmd/astral format`, `lint` and `test` before committing (the pre-commit hook in `.githooks` runs lint and tests); the tests build the ViewWire host test under ASan and UBSan and run every plugin's Lua tests.
- astral is always written lowercase in visible text, docs and commits.
- Commit as `type(scope): subject`, with scope `ios`, `core`, `plugins`, `sdk`, `themes`, `catalog`, `tools` or `docs`.
- Plugins are Lua only; new capabilities go into the `astral` API, never into native plugin code.
- Keep Discord account data, screenshots, IPAs and decrypted binaries out of the repository.
- Everything in `packages/tools` is Go; new tooling becomes an `astral` subcommand, never a shell script.
- Every install, jailed or jailbroken, runs the same code path; never add anything that needs a jailbreak (code patching, ElleKit, `/var/jb` only features).
