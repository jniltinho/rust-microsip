# Approval to start implementation

On 2026-10-12, after resuming commit `14b46b9`, presenting the limits below, and requesting confirmation, the user approved starting implementation (translated from Portuguese: “you may start implementing”).

- Main change: `create-native-rust-linux-release`, starting with G0.
- Linked extension: `add-agent-cli-mcp`, local, authorized, and single-owner; its full implementation depends on G0.
- v0.1: Linux x86_64/Ubuntu 24.04, one enabled SIP account and one simultaneous call.
- ELF is the primary artifact; AppImage is additional and requires separate testing.
- Full-call Web/WASM, macOS/Windows, and advanced integrations remain later releases, as specified in proposal non-goals.
- Proposed own-code license direction is GPL-3.0-or-later; copying upstream resources without auditing is not authorized.

Approval permits implementation/testing, not a declaration that G0 passed, production calls, release publication, or visibility changes. `base/`, personal settings, and the installed AppImage remain untouched.

Subsequent instructions on the same date require English code/comments/CLI/documentation/tasks, commits and pushes at verified milestones, and use of the pinned andrius/asterisk repository/image for the disposable PBX. They do not waive release gates.
