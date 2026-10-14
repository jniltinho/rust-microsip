# Cutting a release

Binaries are built and published only by `.github/workflows/release.yml`, which runs when a tag
`vX.Y.Z` (or `vX.Y.Z-rc.N`) is pushed. `ci.yml` runs the checks on `main` and pull requests and never
uploads binaries. Tagging is the owner's decision.

## Rules enforced by the workflow

1. The tagged commit is **on `main`** and has a **successful `ci.yml` push run on `main`**.
2. The tag equals `version` in `[workspace.package]` of `Cargo.toml`.
3. The job runs in the protected **`release` environment**: it waits until the owner approves it
   on GitHub (required reviewer; only `v*` tags may deploy). A first job refuses to run when that
   environment is missing or lost this protection. An agent can push a tag but cannot publish.
   The workflow file itself lives on `main`, so this relies on only the owner pushing to `main`.
   Admin bypass is disabled. Approval comes from the owner's GitHub account, so never give an
   agent credentials that can approve deployments.
4. A **final** version (`vX.Y.Z`) needs `docs/release/vX.Y.Z.md` (release notes and gate
   checklist) that passes `packaging/release/check-gates.sh X.Y.Z`: no open checkbox in any
   task-list syntax, every task id of the [Linux release change](../openspec/changes/create-native-rust-linux-release/tasks.md)
   and the [CLI/MCP change](../openspec/changes/add-agent-cli-mcp/tasks.md) listed as checked,
   `AG-01`..`AG-06` checked and an `Owner decision` line checked. Each item starts with its own
   label (`- [x] create-native-rust-linux-release 1.1 ...`); `check-gates-selftest.sh` holds the
   negative controls. Never tick a gate without a
   receipt bound to a commit.
5. Only `vX.Y.Z-rc.N` tags are pre-releases (no gate check), for example to carry the exact ELF and
   AppImage into the clean VMs of the acceptance checklist; their notes say they are not a release.
   Any other suffix is refused.

The workflow then builds `docker buildx build --target artifacts` and `--target windows-artifacts`,
checks both `SHA256SUMS`, verifies that the AppImage wraps the identical ELF, runs the publication
scan over the artifacts (zip members and the MSI payload unpacked) and publishes:

- `rust-microsip-X.Y.Z-x86_64` (ELF; needs glibc ≥ 2.39, `libasound.so.2` and, on X11, `libxkbcommon-x11`)
- `rust-microsip-X.Y.Z-x86_64.AppImage`
- `rust-microsip-X.Y.Z-sbom.cdx.json`
- `rust-microsip-X.Y.Z-licenses.tar.gz` (LICENSE, NOTICE, Rust dependency and toolchain notices,
  build manifest)
- `rust-microsip-X.Y.Z-windows-x86_64.zip` (portable: both exes, LICENSE, NOTICE and
  THIRD-PARTY-NOTICES.txt for the Windows dependency graph)
- `rust-microsip-X.Y.Z-windows-sbom.cdx.json` (CycloneDX inventory of the Windows dependency graph)
- `rust-microsip-X.Y.Z-x86_64.msi` (per-user, no elevation, `%LOCALAPPDATA%\Programs\rust-microsip`
  with a Start Menu shortcut; a newer version replaces an older one)
- `SHA256SUMS`

The Windows packages are unsigned: say so in the release notes, with the SmartScreen prompt and the
`Get-FileHash` check.

## Steps

1. Set `version` in `[workspace.package]` of `Cargo.toml`, refresh `Cargo.lock` in Docker
   (`scripts/cargo-docker.sh cargo update -w`), commit and push to `main`.
2. For a final version, write `docs/release/vX.Y.Z.md` (notes, known limitations, rollback, the
   checked gate list with links to receipts) and push it.
3. Wait for `ci` to pass on that commit:

   ```sh
   gh run list --workflow ci.yml --commit "$(git rev-parse HEAD)" -L 1
   ```

4. Tag and push:

   ```sh
   git tag -a vX.Y.Z -m "Release vX.Y.Z" && git push origin vX.Y.Z
   ```

5. Watch `gh run list --workflow release.yml -L 3` and check `gh release view vX.Y.Z`; download the
   assets and run `sha256sum --check SHA256SUMS`.

The `create-release` skill (`.claude/skills/create-release/`) walks through the same steps.

## Local dry run

```sh
make dist DIST=dist/test
make dist-check DIST=dist/test
```

## Rollback

Delete the GitHub Release and the tag (`gh release delete vX.Y.Z --cleanup-tag`), publish a fixed
`vX.Y.Z+1`, and tell users to keep their settings: the database is never downgraded and a newer schema
is refused untouched by older builds.
