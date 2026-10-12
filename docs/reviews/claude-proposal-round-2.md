# Independent OpenSpec proposal review — round 2

Historical planning-only review, 2026-10-12. English summary translation; not a new review or runtime approval.

**VERDICT: APPROVE**

All five previous findings were checked against actual documents; no concrete documentary blockers remain.

## Fix verification

1. **Internal versus host ports:** consistent internal/no-port default, host override moves PBX to non-internal loopback bridge, coherent external signaling/media/port/rport/contact configuration, both-direction SDP validation. Design/DIST-04/05/matrix/task1.5 align.
2. **Clean VM reachability:** G4 auxiliary PBX with host-only LAB_BIND_IP/ACL/firewall, never wildcard; clients remain free of Docker/Rust/extra packages, operator B has reference softphone/headset. Design/DIST-01/04/05/matrix/task9.6 align.
3. **Impairment/NAT:** seeded user-space UDP proxy without tc/CAP_NET_ADMIN for AUD-02; separate disposable NAT-router/PBX/STUN topology for AUD-06, personal host untouched. Default Compose does not claim NAT proof.
4. **G0 stack:** design/task1.8 require production-equivalent eframe/wgpu/audio/D-Bus/notifications/IPC, compatible renderer, focus/keyboard/hotplug, DT_NEEDED/dlopen; failure blocks G0.
5. **KDE/Qt:** no extra desktop packages and Qt-free ELF dependencies, consistent with standard KDE.

## General consistency

Seven specs, 46 scenarios (AUD6/CFG9/CH4/LIN4/ND6/DIST7/SIP10), 46 test rows, 54 tasks (9+5+6+6+5+4+5+4+10). Six-layer precedence matches CFG-01. DND486, WAL/SHM/backup permissions, memory-only literal passwords, CSV rules align. Runtime evidence remains pending.

## Nonblocking observations

Covered by task1.5/9.6 or runtime gates, but make explicit during implementation:
- G4 command and whether overrides stack.
- Compose lists merge; use !reset/!override or non-stacked files for networks/ports.
- Auxiliary firewall must filter Docker forwarding through DOCKER-USER, not only INPUT/UFW; Asterisk ACL complements, not replaces it.
- Teardown repeats the same override files or networks may remain.
- Host DNAT may appear from bridge gateway inside local_net, bypassing external SDP rewriting; verify actual SDP.
- AUD-06 ACL includes NAT-router external IP.
