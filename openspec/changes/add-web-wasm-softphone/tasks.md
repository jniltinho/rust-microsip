## 1. Web gate GW0 after Linux

- [ ] 1.1 Confirm Linux/CLI/MCP completion and approve a separate web release; verify roadmap/scope and no promise of web in v0.1.
- [ ] 1.2 Create apps/web and core/platform-web separation; verify wasm32 without CPAL/SQLite/Secret Service/Linux IPC and Docker builds without a host toolchain (WEB-01, WEB-06).
- [ ] 1.3 Pin/audit egui web, wasm-bindgen, the full SIP.js framework, and the bridge; verify the versions/MSRV/licenses ADR and no implicit CDN/remote resources.
- [ ] 1.4 Create an isolated Asterisk WebRTC/WSS/DTLS lab and CA; verify readiness, host-only binds/ACL, SDP, and no public PSTN/admin routes (WEB-05).
- [ ] 1.5 Create a forced NAT/TURN scenario in isolated VMs with pinned coturn and ephemeral secrets; verify relayed media in both directions and teardown (WEB-05).
- [ ] 1.6 Prove GW0 in Chromium/Firefox: WASM/UI/CSP, registration/calls, G.711, DTMF, hold, REFER/NOTIFY, and permissions; verify versioned receipts and block full UI work if the candidate fails (WEB-01, WEB-02).

## 2. UI, data, and telephony

- [ ] 2.1 Implement compact UI/themes/focus and browser diagnostics; verify approved mock snapshots and keyboard/scaling across all three tabs (WEB-01).
- [ ] 2.2 Implement the WSS/WebRTC adapter and state machine; verify incoming/outgoing calls, rejection/BYE/CANCEL, DND, mute/hold/transfer, and real PBX outcomes (WEB-02).
- [ ] 2.3 Implement audio preparation, permissions, and gracefully degrading device selection; verify autoplay/denial/revocation/hotplug/indicators and no microphone on page load (WEB-03).
- [ ] 2.4 Implement per-tab lifecycle/locking and reconciliation; verify reload/close/suspension, duplicate tabs, no automatic redial/takeover, and best-effort track release (WEB-03).
- [ ] 2.5 Implement non-secret IndexedDB and contact/history consent; verify quota/private mode, import/export/cleanup, absence of passwords/TURN in all stores/logs/URLs/SIP.js console output, and documentation without a zeroization guarantee (WEB-04).
- [ ] 2.6 Implement endpoint/URI validation, CSP/Permissions-Policy, mandatory WSS proxy/Origin checks, and network/TLS errors; verify malicious input, blocked WSS origins, and no certificate bypass (WEB-04, WEB-05).

## 3. Build, deployment, and acceptance

- [ ] 3.1 Complete the Docker bundle with locks/checksums/SBOM/licenses; verify a clone without base, self-hosted assets, and no build/runtime secrets in the bundle (WEB-06).
- [ ] 3.2 Document HTTPS, certificates, PBX/gateway, STUN/TURN, and headers; verify reproducible isolated deployment without a native app or access to personal settings/PBX (WEB-05, WEB-06).
- [ ] 3.3 Run WEB-01..06 with frozen Chromium/Firefox versions, synthetic integration, and two operators/headsets; verify five minutes per agreed codec on the exact bundle, including forced relay (WEB-02, WEB-05, WEB-06).
- [ ] 3.4 Review with Claude CLI, security/dependency auditing, and an operator; verify zero blockers, documented background limitations, and compatible bundle/data rollback before the separate release.
