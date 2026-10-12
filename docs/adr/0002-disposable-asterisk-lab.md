# ADR 0002 — pinned andrius/asterisk test lab

Date: 2026-10-12. Status: accepted for isolated infrastructure only; G0 remains pending.

## Decision and provenance

At the user's explicit request, use [andrius/asterisk](https://github.com/andrius/asterisk) to accelerate the disposable PBX, rather than maintaining a second Asterisk source builder. Reviewed repository revision: `4e421e1cef64dd864f86881f852a173c9b8b7e0d` (2026-10-10).

- Exact image: `andrius/asterisk:22.11.0_debian-trixie`.
- Selected **linux/amd64 platform manifest**: `sha256:eaa6aaaa09ede2829514769d5d1aba3041ec8b3055899feebfc8e8a9a1af7c0f`.
- Runtime check: `Asterisk 22.11.0`, Python 3.13.5, OpenSSL 3.5.7.
- Asterisk 22 is [fully supported LTS through 2028-10-16, with security maintenance through 2029-10-16](https://docs.asterisk.org/About-the-Project/Asterisk-Versions/).
- The [reviewed upstream Dockerfile](https://github.com/andrius/asterisk/blob/4e421e1cef64dd864f86881f852a173c9b8b7e0d/asterisk/22.11.0-trixie/Dockerfile) verifies Asterisk source SHA-256 `3bd5ee040509a3d3cd9b1ba9520c18e6ec0a7e7981ca68c457dcd36ba3c54d94`. Its Debian package inputs and bundled PJProject download were not independently reproduced by us. The image digest fixes the downloaded bytes, not source-to-image provenance or absence of vulnerabilities.

## License and resource boundaries

The repository's orchestration files are MIT licensed; Asterisk itself is GPL-2.0 with the specific OpenSSL and other named permissions in its upstream license (not a blanket linking exception). The upstream image contains third-party system libraries and optional modules/assets with their own notices, including inherited sound/music-on-hold assets that require separate attribution/license review before redistribution. We do **not** incorporate any of them into the Rust client, its ELF/AppImage, or its source/build context. The PBX communicates over SIP/RTP as a separate test process. No upstream fonts, logos, icons, recordings, or UI resources are copied into the application. This is an engineering inventory, not a legal opinion or a complete release SBOM.

The local lab wrapper loads an explicit module allowlist, G.711 only, no optional Opus/G.729, recording, remote administration, outbound trunks, or anonymous endpoint. Test tones are generated mathematically by Asterisk. Image redistribution is not part of the client release; before any future PBX image publication, audit all inherited components and provide their corresponding source/notices as required. Application dependency/license task 1.3 remains open.

## Hardening adaptations

Do not use upstream `latest`, root entrypoint, verbose/debug startup, example host networking, or persistent volume defaults. A fresh scratch stage copies the pinned filesystem to discard inherited `VOLUME`, entrypoint, and healthcheck metadata. Our entrypoint starts directly as a numeric non-root UID, generates configuration in tmpfs, and uses an explicit PJSIP/module healthcheck. The upstream filesystem remains read-only; only private tmpfs and the explicitly selected fictional lab directory are used.

Default topology is internal, without published ports, host networking, privileges, Docker socket, HOME, DBus, devices, or personal configuration. Host loopback is explicit opt-in; that profile uses a non-internal bridge and permits container egress, unlike the default isolated network. CA issuer keys are not mounted into PBX or clients; server keys are not mounted into clients. Runtime credentials are never baked into the image. Teardown removes the PBX but does not delete credentials from disk; passwords have no intrinsic expiry. Delete the ignored lab directory after use. Certificates expire after three days.

## Acceptance limits

Registration fixtures prove PBX readiness and authentication, not the Rust SIP adapter, calls, SDP rewriting, bidirectional media, SRTP negotiation, VM ACLs, or human headset acceptance. Those require separate receipts and must remain unchecked until actually verified.
