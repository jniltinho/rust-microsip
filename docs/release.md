# Cutting a release

Binaries are built and published only by `.github/workflows/release.yml`, which runs when a tag
`vX.Y.Z` (or `vX.Y.Z-rc.N`) is pushed. `ci.yml` runs the checks on `main` and pull requests and never
uploads binaries. Tagging is the owner's decision.

## Rules enforced by the workflow

1. The tagged commit has a **successful `ci.yml` run** (tag only green commits).
2. The tag equals `version` in `[workspace.package]` of `Cargo.toml`.
3. A **final** version (`vX.Y.Z` without a suffix) needs `docs/release/vX.Y.Z.md` — the release
   notes and gate checklist — with **no unchecked `- [ ]` item**. For v0.1.0 that checklist covers
   every gate of the [Linux release change](../openspec/changes/create-native-rust-linux-release/tasks.md),
   the CLI/MCP gates AG-01..06 and the owner's visibility decision (see the
   [v0.1.0 draft](release/v0.1.0-draft.md)).
4. `-rc.N` tags are published as **pre-releases** without the gate check, for example to carry the
   exact ELF and AppImage into the clean VMs of the acceptance checklist. Their notes must say they
   are not a release.

The workflow then builds `docker buildx build --target artifacts`, checks `SHA256SUMS`, verifies that
the AppImage wraps the identical ELF, runs the publication scan over the artifacts and publishes:

- `rust-microsip-X.Y.Z-x86_64` (ELF; needs glibc and `libasound.so.2`)
- `rust-microsip-X.Y.Z-x86_64.AppImage`
- `rust-microsip-X.Y.Z-sbom.cdx.json`
- `rust-microsip-X.Y.Z-licenses.tar.gz` (LICENSE, NOTICE, Rust dependency and toolchain notices,
  build manifest)
- `SHA256SUMS`

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
