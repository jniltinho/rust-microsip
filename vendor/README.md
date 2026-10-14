# Vendored crates

Patched copies of registry crates, wired with `[patch.crates-io]` in the root `Cargo.toml`. The
workspace excludes `vendor/`, so each copy keeps its own manifest and lints. Notices are collected
by `packaging/collect-cargo-notices.py` only for the identity, directory and licence files pinned in
its `VENDORED` table.

## wgpu-hal 30.0.1

- Source: the crates.io archive `wgpu-hal-30.0.1.crate`, SHA-256
  `b6b7fb58561a792bc237628ba0792e332de418fefe145f13b5ed8201e6d52f58` (the checksum the lockfile
  pinned before the patch). Licence: MIT OR Apache-2.0; `LICENSE.MIT` and `LICENSE.APACHE` are
  unchanged.
- Removed from the archive: `examples/` and its three `[[example]]` entries, `Cargo.lock`,
  `Cargo.toml.orig` and `.cargo_vcs_info.json`.
- One code change, `src/auxil/dxgi/factory.rs`, `should_keep_adapter` (Windows/DX12 only):

  ```diff
       if desc.VendorId == 5140
           && !Dxgi::DXGI_ADAPTER_FLAG(desc.Flags as i32).contains(Dxgi::DXGI_ADAPTER_FLAG_SOFTWARE)
  +        && unsafe { adapter.EnumOutputs(0) }.is_err()
       {
  ```

  On a machine with no GPU (VirtualBox without 3D), DXGI lists two Microsoft Basic Render Driver
  (WARP) adapters. Upstream skips the non-software one, but that is the adapter that owns the
  display output. Presenting from the other one leaves the window black, or eframe cannot open
  it. The patch keeps that adapter when it owns an output, so headless machines are unaffected.
  The rustXterm project found and verified this on the shared Windows Server 2022 VM.
- Remove the patch when upstream wgpu-hal changes this filter, and record it here.
