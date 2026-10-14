# Windows packaging receipt, milestone W-C (2026-10-14)

**Result: `make windows` and the `windows-artifacts` Docker stage produce a portable zip, a per-user MSI, `THIRD-PARTY-NOTICES.txt` and a CycloneDX SBOM for the Windows dependency graph. Two builds on one machine have identical hashes. On the `windows-2022` runner, CI installs, upgrades and uninstalls the MSI. The publication scan unpacks zip members and the MSI payload. Signing (4.4) and the user documentation (4.5) remain open. No call has been made on Windows.**

Change: [add-windows-port](../../openspec/changes/add-windows-port/proposal.md). Commits `fe5e0ef`, `96b16e2` and `1f64e40` (tagged `v0.1.0-rc.3`).

## Portable zip (task 4.1)

- `packaging/windows/zip.py` writes the members in a fixed order with a fixed timestamp (2026-01-01) and mode 0644:
  - `rust-microsip.exe`;
  - `rust-microsip-cli.exe`;
  - `LICENSE.txt`;
  - `NOTICE.txt`;
  - `THIRD-PARTY-NOTICES.txt`.
- `SHA256SUMS` covers every file in the output directory.
- Repeated build: `rm -rf dist/windows; make windows` twice gave identical `SHA256SUMS`, including the zip and the MSI.
- `make windows` for `0.1.0-rc.3` (`1f64e40` content) wrote these hashes. The release workflow builds its own copy in the `windows-artifacts` stage, and that copy's hashes are in the release `SHA256SUMS`.
  - `rust-microsip-0.1.0-rc.3-windows-sbom.cdx.json`: `c5d63ea82acefadb26a0561f3f8d6aef40ecc85e9e6acf5f8bdc1e55407014ca`
  - `rust-microsip-0.1.0-rc.3-windows-x86_64.zip`: `510658d649ecc7dc015faaaa4ff48a8efbbe7f380f114fe0896d0585cff6545a`
  - `rust-microsip-0.1.0-rc.3-x86_64.msi`: `17b1c7df1156a89f39eb835f5671c06bed147449aebdbc85b56dd66d25e48b61`
  - `rust-microsip-cli.exe`: `221ff5993376c327d4c32ff9e2ef8abe0c9aa4796738e2daeb712d85a89c3ae7`
  - `rust-microsip.exe`: `d65352185135fc5c75c852e7a72ddb611db7c2e8ae80fc69c6f00a6b7c8ef3c3`

## Per-user MSI (task 4.2)

- `packaging/windows/rust-microsip.wxs`, built with wixl 0.103 from msitools (Ubuntu snapshot `20261001T000000Z`):
  - per-user, installed to `%LOCALAPPDATA%\Programs\rust-microsip`;
  - a Start Menu shortcut carrying the AppUserModelID `rust-microsip`, which is the ID the toasts use; msibuild adds this as a `MsiShortcutProperty` row;
  - `MajorUpgrade`, with a fixed UpgradeCode and fixed component GUIDs.
- Reproducible: wixl ignores `SOURCE_DATE_EPOCH`, so `faketime` fixes its clock; `msibuild -s` sets the PackageCode to a UUIDv5 of the version; the input mtimes are fixed.
- `msi.sh` fails the build in each of these cases:
  - the payload differs from exactly the five inputs, compared byte for byte after `msiextract`;
  - the AUMID row is missing;
  - the summary Word Count is not 10 (compressed, "elevated privileges are not required");
  - `ALLUSERS` is set.
- CI `windows-test` step "Per-user MSI install, upgrade and uninstall" (windows-2022) proves that:
  - `msiexec /i /qn` installs the five files and the Start Menu shortcut;
  - exactly one product is registered under the user's SID in `Installer\UserData`, and none under `S-1-5-18` (per-machine);
  - the installed CLI reports its version;
  - installing a 99.0.0 package replaces the old version, leaving one entry with DisplayVersion `99.0.0`;
  - `msiexec /x` removes the files, the shortcut and the registration;
  - a fictional file in `%APPDATA%\rust-microsip` and a fictional Credential Manager entry `rust-microsip:ci-fictional` are still present after uninstall.
- The runner account is an administrator, so the absence of an elevation prompt rests on the package declaring per-user (Word Count, no `ALLUSERS`). A UAC prompt on a standard account has not been observed.

## Notices, SBOM and scanner (task 4.3)

- `collect-cargo-notices.py` takes `RMS_NOTICE_TARGET=x86_64-pc-windows-gnu`. Its normal/build Windows graph has 236 packages, each with checksum-verified notices. This needed:
  - pinned upstream notices for `accesskit_consumer` 0.35.0, `accesskit_windows` 0.32.1 and `clipboard-win` 5.4.1, at the commits named in their `.cargo_vcs_info.json`;
  - reviewed BSL-1.0 exceptions for `clipboard-win` 5.4.1 and `error-code` 3.4.0;
  - an exact-checksum size pin for `windows` 0.62.2, whose unpacked size is 114 MB.
- Vulkan and GL moved to `cfg(unix)`, so Windows uses DX12 only. This removed `gl_generator`, `khronos_api` and `glutin_wgl_sys` from the Windows graph. `Cargo.lock` is unchanged.
- `packaging/windows/notices.py` adds the notices of the Rust standard library (`COPYRIGHT-library` and its license texts) and the mingw-w64 runtime (libgcc, winpthreads). It also writes `rust-microsip-<version>-windows-sbom.cdx.json` (238 components).
- `scan-publication.sh` unpacks zip members and, through `msiextract`, the MSI payload. Without msitools the scan fails closed on an `.msi`. New negative controls in `ci-selftest.sh` all block:
  - a personal home path inside a zip member;
  - a `.env` member;
  - a fictional token inside a zip member;
  - an `.msi` with no msitools present.
- `release.yml` builds `windows-artifacts`, checks its `SHA256SUMS`, scans `dist` and `dist-windows`, and publishes the zip, the MSI and the Windows SBOM.

## Checks

- Local runs on `96b16e2` content: `make verify`, `make context`, `make selftest` and `make scan` all pass.
- The `windows-artifacts` stage built offline. Its output: both exes, the zip, the MSI, `upgrade-test/rust-microsip-99.0.0-x86_64.msi` and the test executables.
- CI run `38520576630` on `1f64e40`: every job passes. That covers Docker verify, artifacts and the context audit, the publication scan, cargo-deny and the Windows cross build. In the Windows tests job, the steps for checksums, test executables, CLI smoke and "Per-user MSI install, upgrade and uninstall" all pass.
- The first MSI run (`38519303635` on `96b16e2`) installed the files but failed the registration check. It had looked for the product under the `Uninstall` keys, and a per-user MSI is not registered there. The check now reads `Installer\UserData`.
