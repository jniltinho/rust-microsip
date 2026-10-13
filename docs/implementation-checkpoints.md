# Implementation batches and checkpoints

The goal remains the complete first usable native Linux release and its linked
CLI/MCP extension. Batch related implementation, then run the shared regression
and integration checks. Small targeted compile/tests remain useful to diagnose a
failure; full suites and independent reviews belong at coherent milestones, not
after every edit. OpenSpec checkboxes require verified full criteria.

## Current ownership and progress

- Main release: 4/54 tasks verified complete; 50 remain.
- Linked CLI/MCP: 0/14 tasks verified complete; 14 remain.
- Telephony/media: task 1.7, SRTCP, authenticated-source lifecycles and secure controls.
- Parallel Claude UI: task 1.8 native spike source, reviewed resources and exact
  candidate graph; source/state tests are not visible-window or G0 approval.

Counts describe task evidence, not a linear completion percentage or ETA. This
checkpoint is dated 2026-10-13; current task files are authoritative.

## Essential G0 batch: must pass before full application implementation

1. Real UDP/TCP/TLS registration, refresh/unregistration and authentication/
   certificate negatives without retry loops or fallback.
2. Independent, bidirectional PCMA/PCMU PBX media and verified call teardown.
3. Mandatory validated-TLS SDES/SRTP/SRTCP, replay/tamper/downgrade negatives,
   bounded authenticated source/key state and no secret-bearing diagnostics.
4. Negotiated DTMF, hold/resume and REFER/NOTIFY success/failure with preserved
   original audio where the PBX retains the dialog; no success on 202 alone.
5. Exact native graphics/audio/D-Bus/IPC candidate: audited graph/resources,
   visible GNOME/Wayland and KDE/X11 window, keyboard/focus, renderer fallback,
   device/hotplug evidence and native dependency allowlist.

Re-use shared fixtures and run one source/ELF-bound PBX regression matrix per
coherent milestone. Static Claude approval is separate from runtime evidence.
No gate can be satisfied by a disabled button, synthetic state or a generated UI.

## Following implementation batches (after G0)

- Domain state/commands/supervision plus configuration, secrets and persistence.
- Complete call lifecycle/recovery, production devices/audio and secure controls.
- Native screens/settings/themes, contacts/history and Linux integration.
- Shared authorized CLI/MCP services and real exact-artifact acceptance.
- Distribution, VM/human audio, performance/visual matrix and final release audit.

## Explicit retest/refinement backlog

Return to UI spacing/polish after real state/action wiring; keep essential
controls, keyboard/focus/contrast/scaling and honest unavailable states gated.
Run broad fault injection, long-duration calls, full visual matrices and clean-VM
checks at their acceptance checkpoints rather than at every edit. Their OpenSpec
requirements still apply before release. Headset/human two-way audio on the exact
ELF/AppImage cannot be replaced by packet tones or a screenshot.

Do not defer license/native/provider review until after compiling or shipping an
unreviewed candidate. Do not silently remove accessibility or security to obtain
an easier green result. A genuinely incompatible candidate requires an explicit
ADR/scope decision, not a fabricated completion checkbox.
