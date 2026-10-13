# G0 native UI preparation

Date: 2026-10-13. Active change: `create-native-rust-linux-release`, task **1.8**. Main release: **4/54** verified tasks; linked CLI/MCP: **0/14**. Neither task 1.8 nor any complete-UI/release task is completed by this source milestone.

## Scope and honesty

`tests/probes/ui_spike.rs` is an isolated, presentation-only candidate authored through the actual Claude CLI, not an application entry point or evidence that telephony works. It provides a proposed resizable 300 x 460 logical-point window laid out like MicroSIP (tab strip with menu button, number field, keypad, edit row, Call/End, volume rows, compact toggles, bottom status bar), Phone/Calls/Contacts tabs, number entry, 3 x 4 keypad, local bounded search, settings, and session-only light/dark/system selection. Number input rejects invalid edits atomically instead of silently changing the proposed destination. Text labels, a one-time initial number focus, Ctrl+L, and normal egui focus navigation are present in source; target-desktop behavior is not yet verified.

No account is registered, no microphone is opened, and no call, notification, IPC request, database, or personal profile is accessed. The status is always **Backend unavailable - G0 UI preparation**. Calls and Contacts contain no fabricated rows. Call/hangup, DND, mute, hold, transfer, account/audio editing, favorites, and import/add are clearly unavailable and disabled. Every live action also fails through the pure action boundary without changing state. Themes are explicitly session-only; persistence and system-service behavior are not implemented or accepted.

`UiState`, `UiAction`, and `UiError` form an injectable local presentation boundary. Screenshot preparation accepts only explicit `--preview-tab dialpad|calls|contacts` and `--preview-theme light|dark|system` pairs; invalid, duplicate, missing or unknown arguments are rejected without echoing input. These flags inject local empty presentation state, not call or registration fixtures. They do not replace the eventual domain snapshots, bounded command queues, authoritative backend events, or transport/platform adapters. No production dependency, manifest, lockfile, or runtime library was added.

## Exact candidate and compile stop

`tests/probes/ui_spike.Cargo.toml` is a **non-production isolated candidate manifest**. Copying it as `Cargo.toml` beside the source creates its own workspace; do not add it to the application workspace automatically.

- `eframe =0.36.2`, defaults disabled: `wgpu_no_default_features`, `wayland`, `x11`, `accesskit`.
- `wgpu =30.0.0`, defaults disabled: `std`, `parking_lot`, `vulkan`, `gles`, `wgsl`.
- `native-ui` is opt-in. No default fonts, persistence, links, inspection listener, glow renderer, or upstream application icon is selected.
- The exact official eframe archive checksum matches the existing [candidate inventory](../inventory/g0-candidate-crates.json). Its current API is `App::ui(&mut self, &mut egui::Ui, &mut Frame)`, not the older `App::update`. [Versioned eframe documentation](https://docs.rs/eframe/0.36.2/eframe/trait.App.html)

The initial source checkpoint stopped native compilation pending review. A later **separately reviewed isolated non-Tokio preview** was compiled and captured for explicit user visual feedback; see [actual screenshots and comparison](../prints/README.md). This is not production/integrated adoption or task 1.8 acceptance. Do not silently remove accessibility, vendor patches, or accept a larger provider union to get a green window.

### Metadata and selected Linux tree findings

An isolated pinned Docker toolchain resolved the candidate without running build scripts or compiling dependencies. The private working lockfile SHA-256 is `b6a68d189f8a2c459873fd7b8e1192e02371e102197d36277c658ccfd66988b4`. It is research evidence, not the production lockfile or an adopted transitive pin set.

`cargo metadata --filter-platform x86_64-unknown-linux-gnu --features native-ui` reported **243** resolve nodes. A separate frozen `cargo tree --target x86_64-unknown-linux-gnu -e normal,build --features native-ui` contained **226** unique package identities. Metadata features can include cross-target unions; the target-specific tree must be checked before claiming a Linux compile feature is selected.

| Path / exact resolved candidate | Observed feature implication | Outstanding decision |
| --- | --- | --- |
| `eframe 0.36.2 -> egui-winit 0.36.2 -> accesskit_winit 0.32.2 -> accesskit_unix 0.21.1 -> zbus 5.19.0` | `accesskit_winit` defaults enable `async-io`; `accesskit_unix` requests zbus `async-io` directly. Linux tree confirms zbus `async-executor,async-fs,async-io,async-lock,async-process,async-task,blocking`. | This would form an unreviewed async-io + Tokio union when combined with the planned platform/telephony stack, contrary to [ADR 0003](../adr/0003-g0-license-and-native-boundaries.md). The isolated UI itself does not select Tokio. Review architecture/dependencies before integration. |
| `wgpu 30.0.0 -> wgpu-core 30.0.1 -> wgpu-core-deps-windows-linux-android 30.0.1 -> wgpu-hal 30.0.1 -> renderdoc-sys 1.1.0` | Non-WASM wgpu dependency requests `renderdoc,wgsl,portable-atomic` internally; Linux tree confirms `renderdoc` even with direct defaults disabled. | Inspect loader behavior and notices; a selected feature alone does not prove a mandatory host RenderDoc package, and no runtime ELF/dlopen evidence exists yet. |
| `eframe 0.36.2 -> egui-winit 0.36.2 -> arboard 3.6.1 -> image 0.25.10` plus eframe's image dependency | Eframe enables clipboard; arboard requests `image-data`. Metadata shows `bmp,png,tiff`, but the **Linux target tree selects image `png` only**, not BMP/TIFF. | Audit the selected Linux graph and clipboard/native behavior; do not report metadata's cross-target BMP/TIFF union as a Linux dependency. |

Primary normalized manifests in the exact registry archives were inspected. `accesskit_winit` offers a `tokio` feature, but egui-winit enables its defaults, and `accesskit_unix` independently requests zbus `async-io`. Adding Tokio directly is additive and does not remove async-io. No clean feature-only resolution has been established for this exact candidate graph. No patch, alternative version, or policy waiver was applied. [accesskit_winit manifest](https://docs.rs/crate/accesskit_winit/0.32.2/source/Cargo.toml), [accesskit_unix manifest](https://docs.rs/crate/accesskit_unix/0.21.1/source/Cargo.toml), [wgpu manifest](https://docs.rs/crate/wgpu/30.0.0/source/Cargo.toml)

The metadata scan found no package matching ADR 0003's blocked package names, including `epaint_default_fonts`. That bounded name check is not complete license, security, notice, or native-provider approval. The subsequent standalone preview review verified **225 selected registry archives**, **10,072 exact source files**, **433 complete notices**, embedded XML/math terms and **32 build scripts**. Its scoped verdict was `PREVIEW_COMPILE_READY` for this isolated non-Tokio graph only, not production adoption. JSON report SHA-256: `d6dde746c6341fb383a1659199c6fc16d9a83ff4c3415d21af75b9afa3546386`; local report: `artifacts/claude-ui-dependency-review.json`. Production notice policy remains unchanged and still rejects 33 selected declarations; the preview review recorded compatible exact-version choices separately rather than silently broadening the policy.

Material findings: Plasma/Misc protocol wrapper MIT declarations do **not** override bundled LGPL-2.1-or-later XML and other nested terms; complete notices/sources were retained for later distribution review. RenderDoc's Linux `RTLD_NOLOAD` lookup is optional and absence returns `NotAvailable`, not a mandatory extra package. Zbus supports environment-selected process/TCP transports, so the isolated run clears bus/AT-SPI and loader/compiler injection and has no network or host bus. No binary or container is published by this screenshot-only checkpoint.

Ignored research evidence: `artifacts/claude-ui-cargo-metadata.json`, `artifacts/claude-ui-cargo-summary.json`, `artifacts/claude-ui-target-tree.txt`, and `artifacts/claude-ui-work/Cargo.lock`. Only the separate `artifacts/claude-ui-cargo-home` cache was used; no production or other-agent cache was mutated.

## Reviewed font, no host resource reads

The spike's only font is the unmodified **Noto Sans Regular 2.008** from the official upstream revision `ffebf8c1ee449e544955a7e813c54f9b73848eac`. [Pinned font source](https://github.com/notofonts/noto-fonts/blob/ffebf8c1ee449e544955a7e813c54f9b73848eac/hinted/ttf/NotoSans/NotoSans-Regular.ttf), [pinned complete license](https://github.com/notofonts/noto-fonts/blob/ffebf8c1ee449e544955a7e813c54f9b73848eac/LICENSE)

The exact copyright/OFL-1.1 text, original font name-table copyright and embedding metadata were inspected before incorporating bytes. The [source manifest](../../tests/probes/ui-assets/source.json) was written first. The font SHA-256 is `b85c38ecea8a7cfb39c24e395a4007474fa5a4fc864f6ee33309eb4948d232d5`; complete license SHA-256 is `0dab92d0544f7b233403f14b84a663bdbfa746982eda629e7f4f9ffe1b036feb`. `OS/2.fsType=0`. No subsetting, modification, host-font lookup, default egui-font bundle, or runtime font download is used.

The font remains OFL-1.1, not GPL. Every distributed copy must retain the complete copyright/license; it must not be sold separately or presented as upstream endorsement. The spike embeds the complete license and exposes it through `--font-license` and Settings. Font approval is scoped to this isolated spike; production export/license collection still requires integration review. [OFL FAQ](https://openfontlicense.org/ofl-faq/)

## Source authoring provenance

Claude CLI **2.1.296** ran in a neutral temporary directory with tools disabled, safe mode, empty setting sources, hooks and auto-memory disabled, empty strict MCP configuration, slash commands disabled, and no session persistence. Only sanitized own requirements and selected primary API details were supplied; no local-only references, installed application, devices, personal settings, credentials, or personal home directory were included. Output was received through stdout and inspected before writing owned source.

Initial source generation exited **0**, `is_error=false`, `subtype=success`, one turn. Prompt SHA-256: `b408eb989964432a4e2be0593a90c22afc7749554419d2609a5b747e260f69a3`. Raw JSON SHA-256: `07a9d764f4aecc5262371b527cb22d2dccbfe0c6973d1123628cef52a852e179`. A separate tools-disabled refinement adds the reviewed font/license and local search/focus corrections; it also exited **0**, `is_error=false`, `subtype=success`, one turn. Refinement prompt SHA-256: `33b5e110029a2c697221e999d1b8d24579b4bff1fe262cf73d8a5761418b1f38`; raw JSON SHA-256: `e4293b666d805f98e985aa56300a6d9ccca190757b7805bd764afd63d9ecb52b`. Raw prompts/results and exit/hash metadata remain ignored under `artifacts/claude-ui-*`.

A third tools-disabled Claude refinement adds strict preview-only state arguments; it exited **0**, `is_error=false`, one turn. Its prompt SHA-256 is `b6cb6f0d2327601e1bf140439cb3e7e623ed44296370d3cb7cbcce94936f2d99` and raw JSON SHA-256 is `c28ccf4d858287efa823e320163386dd81da05e8f11e5a3a5556f75434691238`.

These calls are **source generation/refinement**, not independent review, runtime approval, functional-call evidence, or release authorization.

## Verified now

- Std-only state/action tests compile with pinned Rust 1.99.0, edition 2024 and warnings denied, in Docker with network disabled and only explicit own source/output mounts. All **15 tests pass**: honest defaults, keypad, atomic invalid/pasted input rejection, leading-plus and length limits, backspace/clear, local tabs/theme, bounded search, rejection of every live action, non-secret errors, empty-font refusal, embedded-license presence, and strict preview-argument/default/state combinations.
- Without `native-ui`, the default executable reports unavailability and exits **1**, not a false successful GUI launch. `--font-license` exits **0** and its output exactly matches both retained copyright notices and the complete OFL text.
- `rustfmt --check` passes for the complete source, including parsing the feature-gated native module; this is not GUI type checking.
- Exact font/license bytes match the source manifest; no upstream application resources or default font bundle were incorporated.
- The exact reviewed native preview now compiles offline/frozen in the pinned snapshot test container. **16/16 native-feature tests** and the own-code warnings-as-errors check pass. The local preview ELF SHA-256 is `4a0acb0ae0fba8fa0ddfe9d6b51d934aff8909d1841fbe57ac0b2eacceff5429`; it is not published.
- **Layout revision 2** (MicroSIP structure, 300 x 460) rebuilt offline/frozen in the same pinned preview image with the same manifest and isolated lockfile (no crate or version added): `rustfmt`, **16/16** native-feature tests and own-code `-D warnings` pass; DT_NEEDED is unchanged (`libgcc_s`, `libm`, `libc`, `ld-linux`). Local ELF SHA-256 `fc29569f4ad609257194c479b4537db96000d86dd584c65c84fe1b81bc174a40` (not published). Six new captures replaced the revision 1 images; per-image hashes are in [capture metadata](../prints/capture.json). The initial Dialpad no longer needs scrolling. Short visible labels on disabled controls carry their full "(unavailable)" text as hover text; density is below the earlier 44-point target and is not approved. Local reproduction: `artifacts/claude-ui-preview-build-capture.sh` (ignored), log `artifacts/claude-ui-preview-build-capture-r3.log`.
- Revision 1: **six actual native client-area screenshots**, Dialpad/Calls/Contacts x light/dark, were captured under private Xvfb with software GLES/llvmpipe and no host display/HOME/audio/D-Bus/GPU/device mounts. See [capture metadata](../prints/capture.json). All final images were visually inspected, without retouching. Revision 1's tall header forced scrolling to reach Call/Hang up; revision 2 fixed that. No compact-fidelity approval is claimed.
- No accessibility-service behavior, target-desktop renderer matrix, audio device, D-Bus notification, IPC identity, or real call was tested. The preview has none of the required production audio/notification/IPC adapters.

The state tests were authored together with source; no test-first RED phase is claimed. Headless state tests are not keyboard/accessibility/visual acceptance.

## Required before production/integrated adoption

- Integrate/export the complete exact selected-registry sources/checksums/notices and compatible license paths; preserve the separately reviewed third-party native/unsafe/build-script inventory. The local preview review does not establish complete distribution compliance.
- Resolve or explicitly review the prohibited provider union without silently changing ADR constraints; audit optional runtime process/loader behavior and preserve accessibility requirements.
- Record the approved complete-stack lock/features and enforce absence of blocked codecs/crypto/default-font packages through feature unification.
- Review actual native/runtime boundaries for Wayland/X11/xkbcommon, graphics loaders/drivers/software rendering, clipboard/accessibility, ALSA, D-Bus and authenticated Unix IPC; never infer absence from an unchanged `ldd` list.
- Compile the reviewed **complete production stack** only in a pinned Docker toolchain with explicit source/cache mounts and offline/frozen operation; no host Cargo or personal desktop/device mounts. The separate screenshot preview followed these isolation controls without pretending to contain that complete stack.

## G0 task 1.8 and visual gates still open

The mandatory spike ELF must include the **same production eframe/wgpu, audio, D-Bus/notifications and IPC v1/SO_PEERCRED stack** as the linked CLI/MCP extension. This UI-only source does not yet do so. Do not mark 1.8 complete from state tests, source generation, or a later standalone window smoke.

| Evidence required | Current status |
| --- | --- |
| Complete-stack ELF and DT_NEEDED/indirect/dlopen audit against reviewed clean-desktop allowlist | Not produced |
| Clean Ubuntu 24.04 GNOME/Wayland and KDE/X11, without extra client packages | Not tested |
| Compatible integrated/software rendering without a dedicated-Vulkan assumption | Not tested |
| Audio enumeration, explicit microphone confirmation, headset hotplug/loss recovery | Not implemented or tested by this source |
| D-Bus absence/notification behavior and IPC v1 peer credentials/ownership | Not implemented or tested by this source |
| Keyboard-only focus/navigation and labelled essential controls, each target desktop | Source preparation only; runtime test pending |
| 1024 x 768 at 100%, 150% and 200%, scrolling without clipped essential controls | Not tested |
| At least 4.5:1 normal-text contrast in light and dark | Not measured |
| Three tabs/search, incoming/active/hold/transfer/errors/settings x light/dark + system | No accepted matrix or snapshots |
| MicroSIP flow comparison and human-approved Rust visual mock/differences | Not approved |
| Real artifact registration/calls/two-way audio and independent read-only Claude review | Separate mandatory gates, not established here |

No full UI rollout, completion percentage, delivery date, or usable release is claimed. If the complete-stack keyboard, renderer, runtime-package or provider gates fail, record the blocker and review the architecture before proceeding; AppImage success cannot replace standalone ELF acceptance.
