# Security policy

astral is pre-release software. Until the first stable release, only the latest commit on `main` is supported.

## Reporting a vulnerability

Report security issues privately through GitHub's [private vulnerability reporting](https://github.com/astral-app/astral/security/advisories/new). Do not open a public issue for a vulnerability.

Include what an attacker can do, the steps to reproduce it, and the affected platform and Discord version. A plugin runs sandboxed Lua and reaches Discord only through the documented `astral` API; a way for a plugin to escape that sandbox, read another plugin's data, or act with the account's token without a granted capability is in scope, as is any way a crafted ViewWire batch, gateway event or server response can run code or corrupt the client.

Please give a reasonable window to fix the issue before disclosing it.
