# Approval record

- **2026-10-14, owner:** "faz primeiro a versão para Windows" ("build the Windows version first"), given after the proposal was presented with its open questions. Implementation starts with the defaults this design recommends:
  - **W1:** `x86_64-pc-windows-gnu` with mingw-w64 in Docker. cargo-xwin/MSVC stays a fallback that needs a separate licence decision.
  - **W2:** `unsafe` only in `crates/platform-windows`, with `// SAFETY:` on every block, enforced by the layering test.
  - **Signing:** none for now; release notes state that builds are unsigned.
  - **Windows 11 evaluation image:** still open; it is decided when task 6.2 starts.
  - **Shared Server 2022 VM:** the rustXterm session agreed. Rules: ask before each session; use only a scratch folder and `rmsip-` task names; touch nothing of rustXterm; leave the VM halted.
- Still open for the owner: code signing (none, OV, EV or Azure Trusted Signing), the optional per-machine MSI with a firewall rule, and the arm64 timing.
