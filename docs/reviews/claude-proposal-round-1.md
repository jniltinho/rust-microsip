# Independent OpenSpec proposal review — round 1

Historical planning-only review, 2026-10-12. English summary translation of findings; no runtime tests were executed.

**VERDICT: REVISE**

Core v0.1/non-goals/native Rust/exceptional FFI/ELF versus AppImage/config precedence/secrets/no-SRTP-downgrade/46-scenario traceability are coherent. The following concrete documentary gaps make acceptance impossible in the described topology; they are not already covered by G0.

## Concrete blockers

1. **Host override does not leave the internal network.** Design §7/DIST-05 add ports but never replace `internal:true`. Define a separate non-internal loopback-bound PBX bridge; validate normalized Compose and no wildcard listening.
2. **Clean VMs cannot reach host-only loopback PBX.** DIST-01/test matrix/G4/task9.6 require VM calls, but installing Docker into clients changes the clean reference. Define an isolated auxiliary-PBX G4 topology as a controlled loopback exception; identify the second human/synthetic endpoint and receipt evidence for both sides.
3. **Impairment/NAT topology conflicts with no privileges.** AUD-02/06 and tasks5.1/5.5 lacked STUN/NAT and assumed privileged tc/masquerade. Choose user-space UDP impairment plus justified dedicated networking services, or document a separate manual topology.
4. **G0 ELF omits UI stack, making audit insufficient.** Task1.8's minimal ELF never opens eframe/wgpu or tests keyboard/AccessKit/fallback. Require production-equivalent UI/audio/D-Bus/notifications, GNOME/Wayland/KDE/X11 opening, fallback renderer, focus/keyboard spike/stop criteria, measured SONAME/dlopen audit.
5. **Qt-free VM contradicts KDE.** Plasma already contains Qt. Say no packages beyond the standard desktop and no Qt dependency in ELF, verified through readelf/dlopen.

## Residual, nonblocking risks

Already covered by G0: rsipstack/Rust-media SIP-SDES maturity, SRTP rekey/telephone-event, audio backend/allowlist selection, GPL/dependency compatibility.

Acceptable implementation risks/clarifications:
- ALSA/CPAL hotplug/stable IDs are fragile; include enumeration/hotplug in G0.
- KDE/KWallet may lack org.freedesktop.secrets; test session fallback.
- Wayland may deny incoming-window focus; notifications become important.
- Distinct transport/encryption/RTP-only negative PJSIP fixtures must be explicit.
- 0600 applies to SQLite WAL/SHM/backups too.
- Clarify explicit --config versus XDG, literal private-TOML passwords, CSV multi-number aggregation/duplicates, DND486 versus603.
- 200 MiB RSS under Mesa/lavapipe may be tight; baseline-first budget is appropriate.
- Reference-sources/validation documents were not supplied here, so AppImage hashes/structural validation were not reviewed.

No runtime execution; planning review only.
